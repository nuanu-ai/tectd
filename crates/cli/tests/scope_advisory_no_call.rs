#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
use sqlx::PgPool;
use support::{id, ready_source_candidate, repository, route};
use tect_postgres::admin;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires disposable PostgreSQL 18.6 and TECT_TEST_*; run with `cargo test -p tect-cli --test scope_advisory_no_call -- --ignored --nocapture`"]
async fn public_scope_advisory_request_records_disabled_no_call_without_dispatch() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    let server_version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(server_version.parse::<i32>().unwrap(), 180_006);

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("scope-advisory-no-call.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-no-call-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;

    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config_path = root.join("host.json");
    host_file(&config_path, &enrollment.auth);
    let workspace_key = format!("scope-no-call-{}", Uuid::new_v4());
    let native_session = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config_path, &native_session, &workspace_key).await;

    let (candidate_context, _) = ready_source_candidate(&mut client, &repo).await;
    let candidate_set_id = id(&candidate_context["candidate_set"]["id"]);
    let config = route(&mut client, "query", "workspace.advisory.config", json!({})).await;
    assert_eq!(config["revision"], 0);
    assert_eq!(config["mode"], "disabled");

    let request_id = Uuid::new_v4();
    let request = route(
        &mut client,
        "command",
        "scope.advisory.request",
        json!({"request_id":request_id,"candidate_set_id":candidate_set_id}),
    )
    .await;
    assert_eq!(request["request_id"], request_id.to_string());
    assert_eq!(request["candidate_set_id"], candidate_set_id.to_string());
    assert_eq!(request["state"], "no_call");
    assert_eq!(request["reason"], "workspace_disabled");
    assert_eq!(request["provider_called"], false);

    let audit = route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":10}),
    )
    .await;
    assert_eq!(audit["opportunities"].as_array().unwrap().len(), 1);
    assert_eq!(audit["opportunities"][0]["id"], request["opportunity_id"]);
    assert_eq!(audit["opportunities"][0]["state"], "no_call");
    assert_eq!(
        audit["opportunities"][0]["primary_reason"],
        "workspace_disabled"
    );
    assert_eq!(audit["dispatches"].as_array().unwrap().len(), 0);
    assert_eq!(audit["aggregate"]["opportunities"], 1);
    assert_eq!(audit["aggregate"]["opportunities_with_attempts"], 0);
    assert_eq!(audit["aggregate"]["no_call_opportunities"], 1);
    assert_eq!(audit["aggregate"]["authorized_attempts"], 0);
    assert_eq!(audit["aggregate"]["confirmed_sent_attempts"], 0);
    assert_eq!(audit["aggregate"]["send_unknown_attempts"], 0);
    assert_eq!(audit["aggregate"]["proven_unsent_attempts"], 0);
    assert_eq!(
        audit["aggregate"]["no_call_by_reason"][0]["reason"],
        "workspace_disabled"
    );
    assert_eq!(audit["aggregate"]["no_call_by_reason"][0]["count"], 1);

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
