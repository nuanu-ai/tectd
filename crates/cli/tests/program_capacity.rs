//! Actual compact Program save and lossless whole-input pagination.
#[allow(dead_code)]
mod recovery_support;

use recovery_support::native_reads::{
    program_lists::read_program_list, program_queries::read_program_query,
};
use recovery_support::{
    Daemon, Mcp, action_name, action_params, host_file, private_temp, public_call, tagged_url,
    tool_payload,
};
use serde_json::json;
use sqlx::PgPool;
use std::time::Instant;
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

fn program_id(payload: &serde_json::Value) -> Uuid {
    Uuid::parse_str(payload["program"]["id"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn escaped_growth_remains_readable_and_large_history_pages_only_whole_inputs() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("capacity.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-program-size-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("program-capacity-{}", Uuid::new_v4().simple());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    client.call("open_workspace", json!({})).await;

    let small = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"small original"}),
        )
        .await;
    let small_id = program_id(&small);
    let before = canonical(&pool, small_id).await;
    // The valid request is below 8 MiB; the compact mutation succeeds and its
    // quote-heavy stored name remains readable through pinned byte fragments.
    let escaped = "\"".repeat(3_000_000);
    let saved_raw = client
        .exchange(
            "tools/call",
            public_call(
                "save_program",
                json!({
                    "program_id":small_id,"revision":1,"input_cursor":1,
                    "name":escaped,"intent":"intent","basis":"basis",
                    "boundaries":"boundaries","constraints":"constraints",
                    "success":"success","complete":true
                }),
            ),
        )
        .await;
    assert!(saved_raw.get("error").is_none() && saved_raw["result"]["isError"] != true);
    assert!(serde_json::to_vec(&saved_raw["result"]).unwrap().len() <= 8192);
    let saved = tool_payload(&saved_raw);
    assert_eq!(saved["program"]["revision"], 2);
    assert_eq!(saved["program"]["status"], "open");
    assert_eq!(saved["program"]["current_step"], "ready");
    assert!(saved["program"].get("name").is_none());
    let listed = read_program_list(
        async |arguments| {
            client
                .exchange("tools/call", json!({"name":"query","arguments":arguments}))
                .await
        },
        json!({"limit":1}),
    )
    .await
    .unwrap();
    let workspace_id = Uuid::parse_str(saved["program"]["workspace_id"].as_str().unwrap()).unwrap();
    assert!(!workspace_id.is_nil());
    assert_eq!(
        listed.initial_query,
        json!({"route":"program.list","params":{"limit":1}})
    );
    assert_eq!(listed.source, Some(json!({"workspace_id":workspace_id})));
    assert!(listed.representation_digest.is_some());
    assert!(listed.pages > 1 && listed.maximum_mcp_bytes <= 8192);
    assert_eq!(listed.value["programs"].as_array().unwrap().len(), 1);
    let summary = &listed.value["programs"][0];
    for field in ["id", "revision", "status", "current_step"] {
        assert_eq!(summary[field], saved["program"][field]);
    }
    assert_eq!(summary["name"], escaped);
    assert_eq!(listed.value["next_after"], serde_json::Value::Null);
    assert_eq!(listed.terminal_actions.len(), 2);
    assert_eq!(listed.terminal_recommended_action, Some(0));
    let byte_query = listed.first_byte_query.as_ref().unwrap();
    let mut wrong_digest = byte_query["params"].clone();
    let digest = wrong_digest["representation_digest"].as_str().unwrap();
    let replacement = if digest.starts_with('0') { "1" } else { "0" };
    wrong_digest["representation_digest"] = json!(format!("{replacement}{}", &digest[1..]));
    let digest_refused = client.call_error("list_programs", wrong_digest).await;
    assert_eq!(digest_refused["error"]["code"], "INPUT_SCHEMA_INVALID");
    assert_eq!(
        digest_refused["error"]["refusal"],
        json!({
            "code":"INPUT_SCHEMA_INVALID",
            "rule":"PIPELINE-JSON-FRAGMENT-REPRESENTATION",
            "path":"arguments.params.representation_digest",
            "expected":"digest of the current authorized JSON representation",
            "actual":"representation changed",
            "next_action":"restart_fragment_at_offset_zero",
            "required":"representation_digest",
            "message":"the submitted value does not satisfy the selected input schema"
        })
    );
    let mut wrong_workspace = byte_query["params"].clone();
    let other_workspace = Uuid::new_v4();
    assert!(!other_workspace.is_nil());
    assert_ne!(other_workspace, workspace_id);
    wrong_workspace["workspace_id"] = json!(other_workspace);
    let workspace_refused = client.call_error("list_programs", wrong_workspace).await;
    assert_eq!(workspace_refused["error"]["code"], "invalid_arguments");
    let after_save = canonical(&pool, small_id).await;
    assert_eq!(
        &after_save[1..],
        &before[1..],
        "original input rows remain immutable"
    );
    let stored = read_program_query(
        async |arguments| {
            client
                .exchange("tools/call", json!({"name":"query","arguments":arguments}))
                .await
        },
        json!({"program_id":small_id}),
    )
    .await
    .unwrap();
    assert_eq!(stored.value["program"]["revision"], 2);
    assert_eq!(stored.value["program"]["name"], escaped);
    assert!(stored.provenance.representation_digest.is_some());

    let mut expected = Vec::new();
    for index in 0..25 {
        let input = format!("{index}:{}", "x".repeat(1_250_000));
        if index == 0 {
            let created = client
                .call(
                    "begin_program",
                    json!({"request_id":Uuid::new_v4(),"input":input}),
                )
                .await;
            expected.push((program_id(&created), input));
        } else {
            let id = expected[0].0;
            client
                .call(
                    "record_program_input",
                    json!({"program_id":id,"request_id":Uuid::new_v4(),"input":input}),
                )
                .await;
            expected.push((id, input));
        }
    }
    let history_id = expected[0].0;
    let mut after = 0_i64;
    let mut actual = Vec::new();
    let mut page_count = 0;
    loop {
        let started = Instant::now();
        let page_read = read_program_query(
            async |arguments| {
                client
                    .exchange("tools/call", json!({"name":"query","arguments":arguments}))
                    .await
            },
            if after == 0 {
                json!({"program_id":history_id})
            } else {
                json!({"program_id":history_id,"after_input":after})
            },
        )
        .await
        .unwrap();
        let page = &page_read.value;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1_000.0;
        page_count += 1;
        let entries = page["inputs"].as_array().unwrap();
        assert!(!entries.is_empty());
        if page_count == 1 {
            eprintln!(
                "default_get_program_25x1250000 elapsed_ms={elapsed_ms:.3} returned_entries={} next_after_input={}",
                entries.len(),
                page["next_after_input"]
            );
        }
        for entry in entries {
            after = entry["sequence"].as_i64().unwrap();
            actual.push(entry["input"].as_str().unwrap().to_owned());
        }
        if page["next_after_input"].is_null() {
            break;
        }
        assert_eq!(page["next_after_input"], after);
        let continuation = page_read
            .provenance
            .terminal_actions
            .iter()
            .find(|action| action_name(action) == Some("program.get"))
            .expect("actual collection continuation");
        assert_eq!(action_params(continuation)["after_input"], after);
        assert_eq!(page_read.provenance.terminal_recommended_action, Some(0));
    }
    assert!(
        page_count > 1,
        "aggregate history must exceed one output frame"
    );
    assert_eq!(actual.len(), 25);
    for (actual, (_, expected)) in actual.iter().zip(&expected) {
        assert_eq!(actual.len(), expected.len());
        assert_eq!(actual, expected);
    }

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
