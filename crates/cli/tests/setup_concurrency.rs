//! Real setup idempotency, revision, and directory-scope concurrency acceptance.

mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url, tool_payload};
use serde_json::{Value, json};
use sqlx::PgPool;
use tect_postgres::admin;
use uuid::Uuid;

fn setup_id(payload: &Value) -> Uuid {
    payload["setup"]["id"].as_str().unwrap().parse().unwrap()
}

async fn canonical(pool: &PgPool, id: Uuid) -> Vec<String> {
    let mut rows = vec![
        sqlx::query_scalar::<_, String>(
            "SELECT xmin::text || ':' || row_to_json(s)::text \
             FROM workspace_setups s WHERE id=$1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap(),
    ];
    rows.extend(
        sqlx::query_scalar::<_, String>(
            "SELECT xmin::text || ':' || row_to_json(i)::text \
             FROM workspace_setup_inputs i WHERE setup_id=$1 ORDER BY sequence",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap(),
    );
    rows
}

async fn inspect(client: &mut Mcp, directory: &std::path::Path) {
    let inspected = client
        .call(
            "inspect_setup",
            json!({"task_directory":directory.to_str().unwrap()}),
        )
        .await;
    assert_eq!(inspected["file"]["status"], "missing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn concurrent_setup_retries_are_scoped_and_stale_saves_are_atomic() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let directory = root.join("shared-task");
    let other_directory = root.join("other-task");
    std::fs::create_dir(&directory).unwrap();
    std::fs::create_dir(&other_directory).unwrap();
    let socket = root.join("concurrency.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-setup-concurrency-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let host = admin::enroll_host_with_grants(
        &pool,
        None,
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let config = root.join("host.json");
    host_file(&config, &host.auth);
    let workspace = format!("setup-concurrency-{}", Uuid::new_v4().simple());
    let mut first = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    let mut second = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    first.call("open_workspace", json!({})).await;
    second.call("open_workspace", json!({})).await;
    inspect(&mut first, &directory).await;
    inspect(&mut second, &directory).await;

    let create_key = Uuid::new_v4();
    let original = "One canonical setup narrative\nwith exact whitespace.  ";
    let (left, right) = tokio::join!(
        first.call(
            "begin_setup",
            json!({"request_id":create_key,"input":original}),
        ),
        second.call(
            "begin_setup",
            json!({"request_id":create_key,"input":original}),
        )
    );
    let id = setup_id(&left);
    assert_eq!(setup_id(&right), id);
    assert_eq!(left["setup"]["revision"], 1);
    assert_eq!(right["setup"]["revision"], 1);
    let creation_state = canonical(&pool, id).await;
    assert_eq!(creation_state.len(), 2);

    let existing = second
        .call_error(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":original}),
        )
        .await;
    assert_eq!(existing["error"]["code"], "setup_exists");
    assert_eq!(existing["actions"].as_array().unwrap().len(), 3);
    assert_eq!(
        existing["actions"][0],
        json!({"tool":"inspect_setup","arguments":{"task_directory":directory}})
    );
    assert_eq!(
        existing["actions"][1],
        json!({"tool":"list_programs","arguments":{"limit":25}})
    );
    assert_eq!(existing["actions"][2]["tool"], "begin_program");
    assert!(existing.get("setup").is_none());
    assert_eq!(canonical(&pool, id).await, creation_state);

    let changed_begin = second
        .call_error(
            "begin_setup",
            json!({"request_id":create_key,"input":"changed creation text"}),
        )
        .await;
    assert_eq!(changed_begin["error"]["code"], "input_conflict");
    assert_eq!(canonical(&pool, id).await, creation_state);

    let answer_key = Uuid::new_v4();
    let answer = "Later original input for the same setup.";
    let recorded = first
        .call(
            "record_setup_input",
            json!({"setup_id":id,"revision":1,"request_id":answer_key,"input":answer}),
        )
        .await;
    assert_eq!(recorded["setup"]["revision"], 2);
    assert_eq!(recorded["setup"]["latest_input"], 2);
    let after_record = canonical(&pool, id).await;
    assert_eq!(after_record.len(), 3);

    let exact_retry = second
        .call(
            "record_setup_input",
            json!({"setup_id":id,"revision":1,"request_id":answer_key,"input":answer}),
        )
        .await;
    assert_eq!(exact_retry["setup"]["revision"], 2);
    assert_eq!(canonical(&pool, id).await, after_record);
    let changed_retry = second
        .call_error(
            "record_setup_input",
            json!({"setup_id":id,"revision":2,"request_id":answer_key,"input":"changed later text"}),
        )
        .await;
    assert_eq!(changed_retry["error"]["code"], "input_conflict");
    assert_eq!(canonical(&pool, id).await, after_record);
    let later_key_cannot_begin = second
        .call_error(
            "begin_setup",
            json!({"request_id":answer_key,"input":answer}),
        )
        .await;
    assert_eq!(later_key_cannot_begin["error"]["code"], "input_conflict");
    assert_eq!(canonical(&pool, id).await, after_record);

    let composed = first
        .call(
            "save_setup",
            json!({
                "setup_id":id,"revision":2,"input_cursor":2,"ready":false,
                "content":"# Shared setup\n\nBoth inputs are incorporated.\n"
            }),
        )
        .await;
    assert_eq!(composed["setup"]["revision"], 3);
    let before_race = canonical(&pool, id).await;
    let left_patch = json!({
        "setup_id":id,"revision":3,"input_cursor":2,"ready":true,
        "content":"# Shared setup\n\nConcurrent result A.\n"
    });
    let right_patch = json!({
        "setup_id":id,"revision":3,"input_cursor":2,"ready":true,
        "content":"# Shared setup\n\nConcurrent result B.\n"
    });
    let (left, right) = tokio::join!(
        first.exchange(
            "tools/call",
            json!({"name":"save_setup","arguments":left_patch}),
        ),
        second.exchange(
            "tools/call",
            json!({"name":"save_setup","arguments":right_patch}),
        )
    );
    let mut success = None;
    let mut refusal = None;
    for response in [&left, &right] {
        let body = tool_payload(response);
        if response["result"]["isError"] == true {
            refusal = Some(body);
        } else {
            success = Some(body);
        }
    }
    let success = success.expect("one revision-checked save succeeds");
    let refusal = refusal.expect("one revision-checked save is stale");
    assert_eq!(success["setup"]["revision"], 4);
    assert_eq!(success["setup"]["current_step"], "ready_to_apply");
    assert_eq!(refusal["error"]["code"], "stale_revision");
    assert_eq!(refusal["actions"][0]["tool"], "get_setup");
    assert_eq!(
        refusal["actions"][0]["arguments"],
        json!({"setup_id":id,"after_input":0,"limit":25})
    );
    assert_eq!(
        refusal["actions"][1],
        json!({"tool":"list_programs","arguments":{"limit":25}})
    );
    assert_eq!(refusal["actions"][2]["tool"], "begin_program");
    let final_state = canonical(&pool, id).await;
    assert_ne!(final_state, before_race);
    assert_eq!(final_state.len(), 3);

    let mut other = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    other.call("open_workspace", json!({})).await;
    inspect(&mut other, &other_directory).await;
    let independent = other
        .call(
            "begin_setup",
            json!({"request_id":create_key,"input":original}),
        )
        .await;
    let independent_id = setup_id(&independent);
    assert_ne!(independent_id, id);
    let independent_answer = other
        .call(
            "record_setup_input",
            json!({
                "setup_id":independent_id,"revision":1,
                "request_id":answer_key,"input":answer
            }),
        )
        .await;
    assert_eq!(independent_answer["setup"]["revision"], 2);
    let independent_begin_retry = other
        .call(
            "begin_setup",
            json!({"request_id":create_key,"input":original}),
        )
        .await;
    assert_eq!(setup_id(&independent_begin_retry), independent_id);
    assert_eq!(independent_begin_retry["setup"]["revision"], 2);
    assert_eq!(canonical(&pool, id).await, final_state);

    first.finish().await;
    second.finish().await;
    other.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
