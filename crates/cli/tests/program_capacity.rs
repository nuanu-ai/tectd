//! Actual 8 MiB adapter-capacity rollback and whole-entry pagination.
mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
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
async fn escaped_growth_rolls_back_and_large_history_pages_only_whole_inputs() {
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
    // This request frame remains below 8 MiB, while the fully escaped successful
    // result would exceed output capacity because the quote-heavy text is encoded twice.
    let escaped = "\"".repeat(3_000_000);
    let refused = client
        .call_error(
            "save_program",
            json!({
                "program_id":small_id,"revision":1,"input_cursor":1,
                "name":escaped,"intent":"intent","basis":"basis",
                "boundaries":"boundaries","constraints":"constraints",
                "success":"success","complete":true
            }),
        )
        .await;
    assert_eq!(refused["error"]["code"], "request_too_large");
    assert_eq!(
        refused["actions"][0],
        json!({"tool":"get_program","arguments":{"program_id":small_id}})
    );
    assert_eq!(canonical(&pool, small_id).await, before);
    let unchanged = client
        .call("get_program", json!({"program_id":small_id}))
        .await;
    assert_eq!(unchanged["program"]["revision"], 1);
    assert!(unchanged["program"]["name"].is_null());

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
        let page = client
            .call(
                "get_program",
                if after == 0 {
                    json!({"program_id":history_id})
                } else {
                    json!({"program_id":history_id,"after_input":after})
                },
            )
            .await;
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
        assert_eq!(page["actions"][0]["tool"], "read_skill");
        assert_eq!(page["actions"][1]["tool"], "get_program");
        assert_eq!(page["actions"][1]["arguments"]["after_input"], after);
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
