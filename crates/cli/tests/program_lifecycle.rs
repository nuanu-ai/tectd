//! Real PostgreSQL/daemon/stdio Program lifecycle and restart acceptance.
mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use tect_postgres::admin;
use uuid::Uuid;

fn assert_actions(payload: &Value) {
    assert!(payload["actions"].is_array(), "{payload}");
    assert!(
        payload["recommended_action"].is_number() || payload["recommended_action"].is_null(),
        "{payload}"
    );
}

fn program_id(payload: &Value) -> Uuid {
    Uuid::parse_str(payload["program"]["id"].as_str().unwrap()).unwrap()
}

async fn canonical(pool: &PgPool, program_id: Uuid) -> (String, Vec<String>) {
    let program = sqlx::query_scalar::<_, String>(
        "SELECT xmin::text || ':' || row_to_json(p)::text FROM programs p WHERE id=$1",
    )
    .bind(program_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let inputs = sqlx::query_scalar::<_, String>(
        "SELECT xmin::text || ':' || row_to_json(i)::text FROM program_inputs i \
         WHERE program_id=$1 ORDER BY sequence",
    )
    .bind(program_id)
    .fetch_all(pool)
    .await
    .unwrap();
    (program, inputs)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rich_program_draft_question_correction_and_restart_preserve_one_record() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("program.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-program-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("program-lifecycle-{}", Uuid::new_v4().simple());
    let first_native = Uuid::new_v4().to_string();
    let second_native = Uuid::new_v4().to_string();
    let mut first = Mcp::start(&socket, &config, &first_native, &workspace).await;
    let mut second = Mcp::start(&socket, &config, &second_native, &workspace).await;
    let first_open = first.call("open_workspace", json!({})).await;
    let second_open = second.call("open_workspace", json!({})).await;

    let request_id = Uuid::new_v4();
    let original = "Build a narrative-first Program.\nKeep `quotes`, \\\\slashes, emoji 🧭, and trailing spaces.  ";
    let created = first
        .call(
            "begin_program",
            json!({"request_id":request_id,"input":original}),
        )
        .await;
    assert_actions(&created);
    let id = program_id(&created);
    let initial = &created["program"];
    assert_eq!(initial["status"], "draft");
    assert_eq!(initial["revision"], 1);
    assert_eq!(initial["current_step"], "compose");
    assert_eq!(initial["input_cursor"], 0);
    assert_eq!(initial["latest_input"], 1);
    for field in [
        "name",
        "intent",
        "basis",
        "boundaries",
        "constraints",
        "success",
        "working_notes",
        "pending_question",
    ] {
        assert!(initial[field].is_null(), "{field}: {initial}");
    }
    assert!(created.get("inputs").is_none());
    assert!(created.get("input").is_none());

    let page = first
        .call("get_program", json!({"program_id":id,"limit":1}))
        .await;
    assert_eq!(page["inputs"].as_array().unwrap().len(), 1);
    assert_eq!(page["inputs"][0]["sequence"], 1);
    assert_eq!(page["inputs"][0]["request_id"], request_id.to_string());
    assert_eq!(page["inputs"][0]["input"], original);
    assert_eq!(page["next_after_input"], Value::Null);
    assert_eq!(page["inputs"][0]["session_id"], first_open["session"]["id"]);
    assert!(page["inputs"][0]["id"].as_str().is_some());

    let question = "Which existing recovery promise is normative?";
    let paused = first
        .call(
            "save_program",
            json!({
                "program_id":id,"revision":1,"input_cursor":1,
                "name":"Program formation","intent":"Capture a coherent PRD",
                "working_notes":"Preserve the current draft while this is unresolved.",
                "pending_question":question
            }),
        )
        .await;
    assert_eq!(paused["program"]["revision"], 2);
    assert_eq!(paused["program"]["current_step"], "waiting_input");
    assert_eq!(paused["program"]["pending_question"], question);
    assert!(paused["program"]["basis"].is_null());

    let answer_id = Uuid::new_v4();
    let exact_answer =
        "Use the existing retry identity byte-for-byte.\nDo not create a second Program.\t";
    let resumed = second
        .call(
            "record_program_input",
            json!({"program_id":id,"request_id":answer_id,"input":exact_answer}),
        )
        .await;
    assert_eq!(resumed["program"]["revision"], 3);
    assert_eq!(resumed["program"]["current_step"], "compose");
    assert_eq!(resumed["program"]["pending_question"], question);
    assert_eq!(resumed["program"]["latest_input"], 2);
    let answer_page = second
        .call(
            "get_program",
            json!({"program_id":id,"after_input":1,"limit":1}),
        )
        .await;
    assert_eq!(
        answer_page["inputs"][0]["session_id"],
        second_open["session"]["id"]
    );

    let completed = second
        .call(
            "save_program",
            json!({
                "program_id":id,"revision":3,"input_cursor":2,
                "basis":"The accepted original inputs are the source basis.",
                "boundaries":"One Program; no Scope or repository mutation.",
                "constraints":"Retain exact input and durable recovery.",
                "success":"The Program resumes and remains editable.",
                "pending_question":null,"complete":true
            }),
        )
        .await;
    assert_eq!(completed["program"]["id"], id.to_string());
    assert_eq!(completed["program"]["status"], "open");
    assert_eq!(completed["program"]["current_step"], "ready");
    assert_eq!(completed["program"]["revision"], 4);

    let before_refusal = canonical(&pool, id).await;
    let refused = first
        .call_error(
            "save_program",
            json!({"program_id":id,"revision":4,"input_cursor":2,"name":null}),
        )
        .await;
    assert_eq!(refused["error"]["code"], "program_incomplete");
    assert_eq!(refused["actions"][0]["tool"], "get_program");
    assert_eq!(refused["actions"][0]["arguments"], json!({"program_id":id}));
    assert_eq!(canonical(&pool, id).await, before_refusal);

    let notes_only = first
        .call(
            "save_program",
            json!({
                "program_id":id,"revision":4,"input_cursor":2,
                "working_notes":null
            }),
        )
        .await;
    assert_eq!(notes_only["program"]["name"], "Program formation");
    assert_eq!(notes_only["program"]["status"], "open");
    assert_eq!(notes_only["program"]["current_step"], "compose");
    assert!(notes_only["program"]["working_notes"].is_null());

    let correction_id = Uuid::new_v4();
    let correction = "Correction: success must include later-session continuation.";
    let corrected = first
        .call(
            "record_program_input",
            json!({"program_id":id,"request_id":correction_id,"input":correction}),
        )
        .await;
    assert_eq!(corrected["program"]["revision"], 6);
    assert_eq!(corrected["program"]["status"], "open");
    assert_eq!(corrected["program"]["current_step"], "compose");
    let ready_again = first
        .call(
            "save_program",
            json!({
                "program_id":id,"revision":6,"input_cursor":3,
                "success":"The Program resumes in a later session and accepts corrections.",
                "complete":true
            }),
        )
        .await;
    assert_eq!(ready_again["program"]["revision"], 7);
    assert_eq!(ready_again["program"]["current_step"], "ready");

    first.finish().await;
    second.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    daemon = Daemon::start(&runtime, socket.clone()).await;
    let later_native = Uuid::new_v4().to_string();
    let mut later = Mcp::start(&socket, &config, &later_native, &workspace).await;
    later.call("open_workspace", json!({})).await;
    let retried = later
        .call(
            "begin_program",
            json!({"request_id":request_id,"input":original}),
        )
        .await;
    assert_eq!(program_id(&retried), id);
    assert_eq!(retried["program"]["revision"], 7);
    let history = later
        .call("get_program", json!({"program_id":id,"after_input":0}))
        .await;
    let exact: Vec<_> = history["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["input"].as_str().unwrap())
        .collect();
    assert_eq!(exact, [original, exact_answer, correction]);
    let default_page = later.call("get_program", json!({"program_id":id})).await;
    assert!(default_page["inputs"].as_array().unwrap().is_empty());
    assert!(default_page["next_after_input"].is_null());
    let skill = later
        .call("read_skill", json!({"name":"tectd-program"}))
        .await;
    assert_eq!(skill["name"], "tectd-program");
    assert!(skill["body"].as_str().is_some_and(|body| !body.is_empty()));
    assert_eq!(skill["actions"], json!([]));
    assert!(skill["recommended_action"].is_null());
    later.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
