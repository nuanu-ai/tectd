use crate::recovery_support::{
    Daemon, Mcp, action_name, action_params, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tect_postgres::admin;
use uuid::Uuid;

const FRAME_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
struct Canonical {
    setup_xmin: String,
    revision: i64,
    input_cursor: i64,
    latest_input: i64,
    max_input_bytes: i64,
    input_count: i64,
    input_xmins: Vec<String>,
}

async fn canonical(pool: &PgPool, setup_id: Uuid) -> Canonical {
    let setup: (String, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT xmin::text,revision,input_cursor,latest_input,max_input_bytes \
         FROM workspace_setups WHERE id=$1",
    )
    .bind(setup_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let input_xmins: Vec<String> = sqlx::query_scalar(
        "SELECT xmin::text FROM workspace_setup_inputs WHERE setup_id=$1 ORDER BY sequence",
    )
    .bind(setup_id)
    .fetch_all(pool)
    .await
    .unwrap();
    Canonical {
        setup_xmin: setup.0,
        revision: setup.1,
        input_cursor: setup.2,
        latest_input: setup.3,
        max_input_bytes: setup.4,
        input_count: input_xmins.len() as i64,
        input_xmins,
    }
}

async fn counts(pool: &PgPool, tenant_id: Uuid) -> (i64, i64) {
    let setups = sqlx::query_scalar("SELECT count(*) FROM workspace_setups WHERE tenant_id=$1")
        .bind(tenant_id)
        .fetch_one(pool)
        .await
        .unwrap();
    let inputs =
        sqlx::query_scalar("SELECT count(*) FROM workspace_setup_inputs WHERE tenant_id=$1")
            .bind(tenant_id)
            .fetch_one(pool)
            .await
            .unwrap();
    (setups, inputs)
}

fn setup_id(payload: &Value) -> Uuid {
    payload["setup"]["id"].as_str().unwrap().parse().unwrap()
}

fn twice_escaped_cost(text: &str) -> usize {
    let once = serde_json::to_string(text).unwrap();
    let twice = serde_json::to_string(&once).unwrap();
    let empty = serde_json::to_string(&"\"\"").unwrap();
    twice.len() - empty.len()
}

async fn page_all(
    client: &mut Mcp,
    id: Uuid,
    content: &str,
    notes: &str,
    step: &str,
) -> (Vec<String>, usize) {
    let mut after = 0;
    let mut originals = Vec::new();
    let mut pages = 0;
    loop {
        let page = client
            .call(
                "get_setup",
                json!({"setup_id":id,"after_input":after,"limit":25}),
            )
            .await;
        pages += 1;
        assert_eq!(page["setup"]["current_step"], step);
        assert_eq!(page["setup"]["content"], content);
        assert_eq!(page["setup"]["working_notes"], notes);
        let inputs = page["inputs"].as_array().unwrap();
        assert!(!inputs.is_empty());
        for input in inputs {
            after = input["sequence"].as_i64().unwrap();
            originals.push(input["input"].as_str().unwrap().to_owned());
        }
        if page["next_after_input"].is_null() {
            break;
        }
        assert_eq!(page["next_after_input"], after);
        assert_eq!(action_name(&page["actions"][0]), Some("tectd-setup"));
        assert_eq!(action_name(&page["actions"][1]), Some("setup.get"));
        assert_eq!(action_params(&page["actions"][1])["after_input"], after);
    }
    (originals, pages)
}

pub async fn run() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let setup_directory = root.join("large-setup");
    let rejected_directory = root.join("rejected-begin");
    std::fs::create_dir(&setup_directory).unwrap();
    std::fs::create_dir(&rejected_directory).unwrap();
    let socket = root.join("capacity.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-setup-size-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host_with_grants(
        &pool,
        None,
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("setup-capacity-{}", Uuid::new_v4().simple());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    client.call("open_workspace", json!({})).await;
    client
        .call("inspect_setup", json!({"task_directory":setup_directory}))
        .await;

    let first = "\"\\\n\t".repeat(250_000);
    assert_eq!(first.chars().count(), 1_000_000);
    let escaped_cost = twice_escaped_cost(&first);
    assert!(escaped_cost > first.len() && escaped_cost < FRAME_BYTES);
    let begun = client
        .call(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":first}),
        )
        .await;
    let id = setup_id(&begun);
    let body = "# Capacity acceptance\n\nKeep every original message whole and exact.\n";
    let notes = "The accepted body and notes must survive every refused patch.";
    let compose = client
        .call(
            "save_setup",
            json!({"setup_id":id,"revision":1,"input_cursor":0,
                "ready":false,"content":body,"working_notes":notes}),
        )
        .await;
    assert_eq!(compose["setup"]["current_step"], "compose");
    assert_eq!(
        page_all(&mut client, id, body, notes, "compose").await.0,
        std::slice::from_ref(&first)
    );

    // Request bytes still fit one frame, but the accepted original and projected body do not.
    let growth = "\"".repeat(2_500_000);
    assert!(
        serde_json::to_vec(&json!({"content":growth}))
            .unwrap()
            .len()
            < FRAME_BYTES
    );
    let before_save = canonical(&pool, id).await;
    let refused_save = client
        .call_error(
            "save_setup",
            json!({"setup_id":id,"revision":compose["setup"]["revision"],
                "input_cursor":0,"ready":false,"content":growth,
                "working_notes":"this refused note must never commit"}),
        )
        .await;
    assert_eq!(refused_save["error"]["code"], "request_too_large");
    assert_eq!(canonical(&pool, id).await, before_save);
    assert_eq!(
        page_all(&mut client, id, body, notes, "compose").await.0,
        std::slice::from_ref(&first)
    );

    let waiting = client
        .call(
            "save_setup",
            json!({"setup_id":id,"revision":compose["setup"]["revision"],
                "input_cursor":0,"ready":false,"pending_question":"Provide more exact context."}),
        )
        .await;
    assert_eq!(waiting["setup"]["current_step"], "waiting_input");
    assert_eq!(
        page_all(&mut client, id, body, notes, "waiting_input")
            .await
            .0,
        std::slice::from_ref(&first)
    );

    let oversized_record = "\"".repeat(3_000_000);
    assert!(
        serde_json::to_vec(&json!({"input":oversized_record}))
            .unwrap()
            .len()
            < FRAME_BYTES
    );
    let before_record = canonical(&pool, id).await;
    let refused_record = client
        .call_error(
            "record_setup_input",
            json!({"setup_id":id,"revision":waiting["setup"]["revision"],
                "request_id":Uuid::new_v4(),"input":oversized_record}),
        )
        .await;
    assert_eq!(refused_record["error"]["code"], "request_too_large");
    assert_eq!(canonical(&pool, id).await, before_record);

    let mut originals = vec![first];
    let mut revision = waiting["setup"]["revision"].as_i64().unwrap();
    for index in 1..7 {
        let input = format!("{index}:{}", "x".repeat(1_150_000));
        let recorded = client
            .call(
                "record_setup_input",
                json!({"setup_id":id,"revision":revision,
                    "request_id":Uuid::new_v4(),"input":input}),
            )
            .await;
        revision = recorded["setup"]["revision"].as_i64().unwrap();
        originals.push(input);
    }
    let ready = client
        .call(
            "save_setup",
            json!({"setup_id":id,"revision":revision,"input_cursor":7,
                "ready":true,"pending_question":null}),
        )
        .await;
    revision = ready["setup"]["revision"].as_i64().unwrap();
    let (ready_inputs, ready_pages) =
        page_all(&mut client, id, body, notes, "ready_to_apply").await;
    assert!(ready_pages > 1);
    assert_eq!(ready_inputs, originals);
    client
        .call("apply_setup", json!({"setup_id":id,"revision":revision}))
        .await;
    let (complete_inputs, complete_pages) =
        page_all(&mut client, id, body, notes, "complete").await;
    assert!(complete_pages > 1);
    assert_eq!(complete_inputs, originals);

    let mut rejected = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    rejected.call("open_workspace", json!({})).await;
    rejected
        .call(
            "inspect_setup",
            json!({"task_directory":rejected_directory}),
        )
        .await;
    let before_begin = counts(&pool, enrollment.tenant_id).await;
    let existing_before_begin = canonical(&pool, id).await;
    let oversized_begin = "\"".repeat(3_000_000);
    assert!(
        serde_json::to_vec(&json!({"input":oversized_begin}))
            .unwrap()
            .len()
            < FRAME_BYTES
    );
    let refused_begin = rejected
        .call_error(
            "begin_setup",
            json!({"request_id":Uuid::new_v4(),"input":oversized_begin}),
        )
        .await;
    assert_eq!(refused_begin["error"]["code"], "request_too_large");
    assert_eq!(counts(&pool, enrollment.tenant_id).await, before_begin);
    assert_eq!(canonical(&pool, id).await, existing_before_begin);
    rejected.finish().await;
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
