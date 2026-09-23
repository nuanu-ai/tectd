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
async fn public_scope_advisory_request_audits_disabled_and_optional_skip_without_dispatch() {
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
    let workspace_id = id(&config["workspace_id"]);

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

    let configured = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({"expected_revision":0,"mode":"optional"}),
    )
    .await;
    assert_eq!(configured["workspace_id"], workspace_id.to_string());
    assert_eq!(configured["revision"], 1);
    assert_eq!(configured["mode"], "optional");

    let skipped_id = Uuid::new_v4();
    let skipped = route(
        &mut client,
        "command",
        "scope.advisory.request",
        json!({
            "request_id": skipped_id,
            "candidate_set_id": candidate_set_id,
            "request_preference": "skip"
        }),
    )
    .await;
    assert_eq!(skipped["request_id"], skipped_id.to_string());
    assert_eq!(skipped["state"], "no_call");
    assert_eq!(skipped["reason"], "request_skip");
    assert_eq!(skipped["provider_called"], false);

    let (session_id, actor_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT s.id,h.principal_id FROM agent_sessions s JOIN hosts h \
         ON h.tenant_id=s.tenant_id AND h.id=s.host_id \
         WHERE s.tenant_id=$1 AND s.native_session_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(&native_session)
    .fetch_one(&pool)
    .await
    .unwrap();
    let skipped_audit = route(
        &mut client,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set_id,"opportunity_id":skipped["opportunity_id"]}),
    )
    .await;
    let opportunity = &skipped_audit["opportunity"];
    assert_eq!(opportunity["workspace_id"], workspace_id.to_string());
    assert_eq!(opportunity["work_item_id"], candidate_set_id.to_string());
    assert_eq!(opportunity["session_id"], session_id.to_string());
    assert_eq!(opportunity["authorized_actor_id"], actor_id.to_string());
    assert_eq!(opportunity["request_key"], skipped_id.to_string());
    assert_eq!(opportunity["config_revision"], 1);
    assert_eq!(opportunity["session_preference"], "use_workspace");
    assert_eq!(opportunity["request_preference"], "skip");
    assert_eq!(opportunity["state"], "no_call");
    assert_eq!(opportunity["primary_reason"], "request_skip");
    assert!(skipped_audit["dispatches"].as_array().unwrap().is_empty());
    assert!(skipped_audit.get("scope_decomposition").is_none());
    let dispatch_attempt_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch \
         WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(id(&skipped["opportunity_id"]))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dispatch_attempt_rows, 0,
        "explicit skip must persist no dispatch or provider-attempt row"
    );

    let skipped_page = route(
        &mut client,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set_id,"limit":10,"reason":"request_skip"}),
    )
    .await;
    assert_eq!(skipped_page["opportunities"].as_array().unwrap().len(), 1);
    assert_eq!(
        skipped_page["opportunities"][0]["id"],
        skipped["opportunity_id"]
    );
    assert!(skipped_page["dispatches"].as_array().unwrap().is_empty());
    assert_eq!(skipped_page["aggregate"]["no_call_opportunities"], 1);
    assert_eq!(skipped_page["aggregate"]["authorized_attempts"], 0);
    assert_eq!(skipped_page["aggregate"]["confirmed_sent_attempts"], 0);
    assert_eq!(skipped_page["aggregate"]["send_unknown_attempts"], 0);

    let history: Vec<(i64, String)> = sqlx::query_as(
        "SELECT revision,mode FROM advisory_workspace_config_history \
         WHERE tenant_id=$1 AND workspace_id=$2 ORDER BY revision",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(history, [(0, "disabled".into()), (1, "optional".into())]);

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
