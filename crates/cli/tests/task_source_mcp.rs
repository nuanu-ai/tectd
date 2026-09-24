//! Real tectd-mcp -> tectd -> PostgreSQL acceptance for Engineering Matrix task sources.
//! This test migrates and writes fixtures. Run it only against a disposable PG18 database.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::route;
use tect_postgres::admin;
use uuid::Uuid;

fn source_input(mode: &str, criticality: &str) -> Value {
    let absent = json!({"state":"absent"});
    json!({
        "mode":{"state":"known","value":mode,"provenance":"owner task brief"},
        "envelope":{"scale":absent,"operational_facts":{"state":"absent"}},
        "criticality":{"state":"known","value":criticality,"provenance":"owner task brief"},
        "intent":absent,"urgency":absent,"promised_behavior":absent,
        "promised_proof":absent,"affected_guarantees":absent,
        "actual_exposure":absent,"demand_commitment":absent,
        "latency_commitment":absent,"urgent_repair":absent
    })
}

fn record(task_id: Uuid, revision: i64, request_id: Uuid, input: Value) -> Value {
    json!({
        "task_id":task_id,"revision":revision,
        "expected_current_revision":revision - 1,
        "request_id":request_id,"input":input
    })
}

fn assert_receipt(receipt: &Value, task_id: Uuid, revision: i64, request_id: Uuid, input: &Value) {
    assert_eq!(receipt["task_id"], json!(task_id));
    assert_eq!(receipt["revision"], revision);
    assert_eq!(receipt["request_id"], json!(request_id));
    assert_eq!(&receipt["input"], input);
    let digest = receipt["input_digest"].as_str().expect("source digest");
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert!(Uuid::parse_str(receipt["recorded_by_principal_id"].as_str().unwrap()).is_ok());
    assert!(Uuid::parse_str(receipt["recorded_by_session_id"].as_str().unwrap()).is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "MIGRATES AND WRITES FIXTURES; requires disposable PostgreSQL 18 and TECT_TEST_DISPOSABLE_PG=1 plus TECT_TEST_* URLs/role"]
async fn task_source_revisions_replay_conflict_staleness_and_workspace_isolation() {
    assert_eq!(
        std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(),
        Ok("1"),
        "explicit disposable-PostgreSQL opt-in required before connecting"
    );
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let version: i32 = sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        version / 10_000 == 18,
        "requires PostgreSQL 18, got {version}"
    );
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("task-source.sock");
    let runtime = tagged_url(&runtime_url, &format!("task-source-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);

    let owner_key = format!("matrix-owner-{}", Uuid::new_v4());
    let mut owner = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &owner_key).await;
    let opened = owner.call("open_workspace", json!({})).await;
    let owner_session_id = opened["session"]["id"].clone();
    let task_id = Uuid::new_v4();
    let rev1_request_id = Uuid::new_v4();
    let rev1_input = source_input("mvp", "Customer-facing preview");
    let rev1_params = record(task_id, 1, rev1_request_id, rev1_input.clone());
    let rev1 = route(
        &mut owner,
        "command",
        "task.source.record",
        rev1_params.clone(),
    )
    .await;
    assert_receipt(&rev1, task_id, 1, rev1_request_id, &rev1_input);
    assert_eq!(rev1["recorded_by_session_id"], owner_session_id);
    let current1 = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(
        current1, rev1,
        "first current read must return the recorded revision"
    );

    let rev2_request_id = Uuid::new_v4();
    let rev2_input = source_input("production", "Payment-facing preview");
    let rev2 = route(
        &mut owner,
        "command",
        "task.source.record",
        record(task_id, 2, rev2_request_id, rev2_input.clone()),
    )
    .await;
    assert_receipt(&rev2, task_id, 2, rev2_request_id, &rev2_input);
    assert_ne!(rev2["input_digest"], rev1["input_digest"]);
    let current2 = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(current2, rev2, "current read must advance to revision 2");

    let replay = route(
        &mut owner,
        "command",
        "task.source.record",
        rev1_params.clone(),
    )
    .await;
    assert_eq!(
        replay, rev1,
        "exact revision 1 replay must return its original receipt"
    );
    let mut changed_replay = rev1_params;
    changed_replay["input"]["criticality"]["value"] = json!("Changed after receipt");
    let conflict = owner
        .call_error(
            "command",
            json!({"route":"task.source.record","params":changed_replay}),
        )
        .await;
    assert_eq!(conflict["error"]["code"], "input_conflict");

    let stale = owner
        .call_error(
            "command",
            json!({"route":"task.source.record","params":record(task_id, 2, Uuid::new_v4(), rev2_input)}),
        )
        .await;
    assert_eq!(stale["error"]["code"], "stale_revision");
    let after_refusals = route(
        &mut owner,
        "query",
        "task.source.get",
        json!({"task_id":task_id}),
    )
    .await;
    assert_eq!(after_refusals, rev2);

    let other_key = format!("matrix-other-{}", Uuid::new_v4());
    let mut other = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &other_key).await;
    let other_open = other.call("open_workspace", json!({})).await;
    assert_ne!(other_open["workspace"]["id"], opened["workspace"]["id"]);
    let hidden = other
        .call_error(
            "query",
            json!({"route":"task.source.get","params":{"task_id":task_id}}),
        )
        .await;
    assert_eq!(hidden["error"]["code"], "not_found");

    other.finish().await;
    owner.finish().await;
}
