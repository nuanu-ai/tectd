//! Real concurrent Program idempotency, pagination, and atomic refusal acceptance.
#[allow(dead_code)]
mod recovery_support;

use recovery_support::{
    Daemon, Mcp, action_name, action_params, host_file, private_temp, public_call, ready_action,
    tagged_url, tool_payload,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::BTreeSet;
use tect_postgres::admin;
use uuid::Uuid;

async fn canonical(pool: &PgPool, id: Uuid) -> Vec<String> {
    let mut rows = vec![
        sqlx::query_scalar::<_, String>(
            "SELECT xmin::text || ':' || row_to_json(p)::text FROM programs p WHERE id=$1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap(),
    ];
    rows.extend(
        sqlx::query_scalar::<_, String>(
            "SELECT xmin::text || ':' || row_to_json(i)::text FROM program_inputs i \
             WHERE program_id=$1 ORDER BY sequence",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap(),
    );
    rows
}

fn id(payload: &Value) -> Uuid {
    Uuid::parse_str(payload["program"]["id"].as_str().unwrap()).unwrap()
}

fn complete_patch(id: Uuid, revision: i64, suffix: &str) -> Value {
    json!({
        "program_id":id,"revision":revision,"input_cursor":3,
        "name":format!("Concurrent Program {suffix}"),
        "intent":"Retain both concurrent inputs",
        "basis":"Original history",
        "boundaries":"This Program only",
        "constraints":"Compare and save",
        "success":format!("Exactly one save wins: {suffix}"),
        "pending_question":null,"complete":true
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn concurrent_inputs_and_saves_are_durable_idempotent_and_atomic() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("concurrency.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-program-race-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("program-race-{}", Uuid::new_v4().simple());
    let mut first = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    let mut second = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    first.call("open_workspace", json!({})).await;
    second.call("open_workspace", json!({})).await;

    let create_key = Uuid::new_v4();
    let original = "initial bytes\nwith \"escaping\" and NUL-like text \\u0000";
    let created = first
        .call(
            "begin_program",
            json!({"request_id":create_key,"input":original}),
        )
        .await;
    let program_id = id(&created);
    let creation_state = canonical(&pool, program_id).await;
    let replay = second
        .call(
            "begin_program",
            json!({"request_id":create_key,"input":original}),
        )
        .await;
    assert_eq!(id(&replay), program_id);
    assert_eq!(replay["program"]["revision"], 1);
    assert_eq!(canonical(&pool, program_id).await, creation_state);
    let conflict = second
        .call_error(
            "begin_program",
            json!({"request_id":create_key,"input":"changed bytes"}),
        )
        .await;
    assert_eq!(conflict["error"]["code"], "input_conflict");
    assert_eq!(action_name(&conflict["actions"][0]), Some("get_state"));
    assert_eq!(canonical(&pool, program_id).await, creation_state);

    let first_key = Uuid::new_v4();
    let second_key = Uuid::new_v4();
    let first_input = "correction from session A\nexact trailing spaces  ";
    let second_input = "correction from session B: 🧪 \\\\ \"quoted\"";
    let (a, b) = tokio::join!(
        first.call(
            "record_program_input",
            json!({"program_id":program_id,"request_id":first_key,"input":first_input}),
        ),
        second.call(
            "record_program_input",
            json!({"program_id":program_id,"request_id":second_key,"input":second_input}),
        )
    );
    let revisions: BTreeSet<_> = [
        a["program"]["revision"].as_i64().unwrap(),
        b["program"]["revision"].as_i64().unwrap(),
    ]
    .into_iter()
    .collect();
    assert_eq!(revisions, BTreeSet::from([2, 3]));
    let after_inputs = canonical(&pool, program_id).await;
    assert_eq!(after_inputs.len(), 4);

    let replay_a = first
        .call(
            "record_program_input",
            json!({"program_id":program_id,"request_id":first_key,"input":first_input}),
        )
        .await;
    let replay_b = second
        .call(
            "record_program_input",
            json!({"program_id":program_id,"request_id":second_key,"input":second_input}),
        )
        .await;
    assert_eq!(replay_a["program"]["revision"], 3);
    assert_eq!(replay_b["program"]["revision"], 3);
    assert_eq!(canonical(&pool, program_id).await, after_inputs);
    let changed_replay = first
        .call_error(
            "record_program_input",
            json!({"program_id":program_id,"request_id":first_key,"input":"not the same"}),
        )
        .await;
    assert_eq!(changed_replay["error"]["code"], "input_conflict");
    assert_eq!(
        changed_replay["actions"][0],
        ready_action("get_program", json!({"program_id":program_id}))
    );
    assert_eq!(canonical(&pool, program_id).await, after_inputs);

    let mut after = 0;
    let mut seen = Vec::new();
    loop {
        let page = first
            .call(
                "get_program",
                json!({"program_id":program_id,"after_input":after,"limit":1}),
            )
            .await;
        assert_eq!(page["inputs"].as_array().unwrap().len(), 1);
        let entry = &page["inputs"][0];
        after = entry["sequence"].as_i64().unwrap();
        seen.push(entry["input"].as_str().unwrap().to_owned());
        if page["next_after_input"].is_null() {
            break;
        }
        assert_eq!(page["next_after_input"], after);
        assert_eq!(action_name(&page["actions"][0]), Some("tectd-program"));
        assert_eq!(action_name(&page["actions"][1]), Some("program.get"));
        assert_eq!(action_params(&page["actions"][1])["after_input"], after);
    }
    assert_eq!(seen[0], original);
    assert_eq!(seen.len(), 3);
    assert!(seen.contains(&first_input.to_owned()));
    assert!(seen.contains(&second_input.to_owned()));

    let before_pending = canonical(&pool, program_id).await;
    let pending = first
        .call_error(
            "save_program",
            json!({
                "program_id":program_id,"revision":3,"input_cursor":1,
                "name":"Too early","intent":"x","basis":"x","boundaries":"x",
                "constraints":"x","success":"x","complete":true
            }),
        )
        .await;
    assert_eq!(pending["error"]["code"], "input_pending");
    assert_eq!(
        pending["actions"][0],
        ready_action("get_program", json!({"program_id":program_id}))
    );
    assert_eq!(canonical(&pool, program_id).await, before_pending);

    let (left, right) = tokio::join!(
        first.exchange(
            "tools/call",
            public_call("save_program", complete_patch(program_id, 3, "A")),
        ),
        second.exchange(
            "tools/call",
            public_call("save_program", complete_patch(program_id, 3, "B")),
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
    let success = success.expect("one concurrent save succeeds");
    let refusal = refusal.expect("one concurrent save is stale");
    assert_eq!(success["program"]["revision"], 4);
    assert_eq!(success["program"]["status"], "open");
    assert_eq!(success["program"]["current_step"], "ready");
    assert_eq!(refusal["error"]["code"], "stale_revision");
    assert_eq!(
        refusal["actions"][0],
        ready_action("get_program", json!({"program_id":program_id}))
    );
    let final_rows = canonical(&pool, program_id).await;
    assert_eq!(final_rows.len(), 4);

    // Original-input retry identities are scoped to their Program.
    let second_program = first
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"independent Program"}),
        )
        .await;
    let second_program_id = id(&second_program);
    let reused_input_key = first
        .call(
            "record_program_input",
            json!({
                "program_id":second_program_id,"request_id":first_key,
                "input":"the key belongs to this Program independently"
            }),
        )
        .await;
    assert_eq!(reused_input_key["program"]["revision"], 2);
    assert_eq!(reused_input_key["program"]["latest_input"], 2);
    assert_eq!(canonical(&pool, program_id).await, final_rows);

    // The same creation retry key belongs to a workspace, not a global namespace.
    let other_workspace = format!("program-race-other-{}", Uuid::new_v4().simple());
    let mut other = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &other_workspace,
    )
    .await;
    other.call("open_workspace", json!({})).await;
    let other_program = other
        .call(
            "begin_program",
            json!({"request_id":create_key,"input":original}),
        )
        .await;
    assert_ne!(id(&other_program), program_id);

    first.finish().await;
    second.finish().await;
    other.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
