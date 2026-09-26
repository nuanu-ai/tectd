//! One native S05 POST against a loopback server, never a candidate-model call.
use super::*;
#[path = "model_route_native/setup.rs"]
pub(super) mod setup;
#[path = "../anti_bloat_native_mcp/source.rs"]
mod source;
#[path = "model_route_native/transport.rs"]
mod transport;
use ring::signature::{Ed25519KeyPair, KeyPair};
use tect_application::{Store, TransactionMode};
use tect_domain::{
    MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA, ModelRoute,
    ModelRouteCatalogue, ModelRouteHostCapabilities,
};
use tokio::net::TcpListener;
type NativeAudit = (
    Vec<u8>,
    String,
    Vec<u8>,
    String,
    Option<i32>,
    String,
    Option<i64>,
);

async fn identity(pool: &PgPool) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let row:(String,i64,String,i64)=sqlx::query_as("SELECT current_database(),d.oid::bigint,(SELECT system_identifier::text FROM pg_control_system()),(SELECT max(version) FROM _sqlx_migrations) FROM pg_database d WHERE datname=current_database()")
        .fetch_one(pool).await.unwrap();
    assert_eq!(
        row,
        (
            "tect_test".into(),
            16385,
            "7689676854994613066".into(),
            decomposition_parent::OWNED_MIGRATION
        )
    );
}
async fn call(pool: &PgPool, client: &mut Mcp, kind: &str, name: &str, params: Value) -> Value {
    identity(pool).await;
    route(client, kind, name, params).await
}
fn catalogue() -> ModelRouteCatalogue {
    ModelRouteCatalogue {
        schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
        version: 1,
        routes: ["route-a", "route-b"]
            .into_iter()
            .map(|id| ModelRoute {
                id: id.into(),
                provider: "candidate.invalid".into(),
                model: format!("candidate-{id}"),
                effort: "medium".into(),
                enabled: true,
                allowed_matrix_choice_ids: vec!["b".into()],
                allowed_roles: vec!["agent".into()],
                allowed_tools: vec!["code".into()],
                allowed_data_classes: vec!["internal".into()],
                required_host_capabilities: vec!["model-api".into()],
                minimum_budget_units: 10,
                minimum_latency_ms: 50,
            })
            .collect(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires explicitly owned disposable PG18.6, migration103"]
async fn native_public_model_route_recommends_once_without_candidate_execution() {
    exercise(false).await;
    exercise(true).await;
}

async fn exercise(duplicate: bool) {
    let pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    identity(&pool).await;
    let runtime = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    support::repository(&repo);
    let socket = root.join("fixture.sock");
    let store = PgStore::connect(&runtime, 4).await.unwrap();
    // Only this synthetic evidence validator is fixture authority. No Matrix
    // provider is installed: selection remains explicit after durable no-call.
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store.clone()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence(Arc::new(AtomicBool::new(false))))),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    identity(&pool).await;
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrolled.auth);
    let native = Uuid::new_v4().to_string();
    let workspace_key = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(&socket, &config, &native, &workspace_key).await;
    let opened = call(&pool, &mut owner, "command", "workspace.open", json!({})).await;
    let workspace = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    call(&pool,&mut owner,"command","workspace.advisory.configure",json!({"expected_revision":0,"mode":"optional","provider_profile_ref":{"id":"fixture-model-route"},"model_configuration":{"model":"fixture-choice-adviser"}})).await;
    let request = setup::lineage(
        &pool,
        &mut owner,
        &store,
        &enrolled,
        &socket,
        &root,
        &config,
        &workspace_key,
        workspace,
        &repo,
    )
    .await;
    let keys = setup::budget(&pool, &store, &enrolled.auth, enrolled.tenant_id, workspace).await;
    owner.finish().await;
    server.abort();
    let _ = server.await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let native_socket = root.join("native.sock");
    let mut daemon = transport::daemon(&runtime, &native_socket, &endpoint, &root, &keys).await;
    let mut owner = Mcp::start(&native_socket, &config, &native, &workspace_key).await;
    let prepared = call(
        &pool,
        &mut owner,
        "command",
        "model.route.prepare",
        request.clone(),
    )
    .await;
    assert_eq!(prepared["preparation"], "Prepared", "{prepared}");
    assert_eq!(prepared["routes"]["requested_route_id"], "route-a");
    assert!(prepared["routes"]["recommended_route_id"].is_null());
    assert!(prepared["routes"]["observed_actual"].is_null());
    let key = request["request_key"].as_str().unwrap();
    let http = tokio::spawn(transport::response(
        listener,
        pool.clone(),
        workspace,
        key.to_owned(),
        duplicate,
    ));
    let run = call(
        &pool,
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":key}),
    )
    .await;
    if duplicate {
        assert_eq!(run["attempt"]["state"], "budget_exhausted", "{run}");
        assert!(run["decision"].is_null(), "{run}");
    } else {
        assert_eq!(run["attempt"]["state"], "parsed", "{run}");
        assert_eq!(run["decision"]["routes"]["requested_route_id"], "route-a");
        assert_eq!(run["decision"]["routes"]["recommended_route_id"], "route-b");
        assert!(run["decision"]["routes"]["observed_actual"].is_null());
        assert_eq!(
            run["decision"]["input"]["Ranking"]["ranked_route_ids"],
            json!(["route-b", "route-a"])
        );
    }
    let replay = call(
        &pool,
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":key}),
    )
    .await;
    assert_eq!(
        run["attempt"]["attempt_id"],
        replay["attempt"]["attempt_id"]
    );
    assert_eq!(run["decision"], replay["decision"]);
    let (request_bytes, raw, listener) = http.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), listener.accept())
            .await
            .is_err(),
        "replay must not POST"
    );
    let attempt = Uuid::parse_str(run["attempt"]["attempt_id"].as_str().unwrap()).unwrap();
    let audit:NativeAudit=sqlx::query_as("SELECT request_payload,request_sha256,response_payload,response_sha256,response_http_status,adapter_identity,response_original_elapsed_ms FROM model_route_advisory_attempts WHERE id=$1").bind(attempt).fetch_one(&pool).await.unwrap();
    assert_eq!(audit.0, request_bytes);
    assert_eq!(audit.1, format!("{:x}", Sha256::digest(&request_bytes)));
    assert_eq!(audit.2, raw);
    assert_eq!(audit.3, format!("{:x}", Sha256::digest(&raw)));
    assert_eq!(audit.4, Some(200));
    assert_eq!(audit.5, "tect.model-route-typesafe-choice/1");
    assert!(audit.6.is_some_and(|n| n >= 0));
    let usage:(i64,i64,Option<i64>,Option<i64>,bool)=sqlx::query_as("SELECT (SELECT count(*) FROM model_route_budget_reservations WHERE attempt_id=$1),(SELECT count(*) FROM model_route_budget_consumptions WHERE attempt_id=$1),input_tokens,output_tokens,unknown_usage FROM model_route_budget_consumptions WHERE attempt_id=$1").bind(attempt).fetch_one(&pool).await.unwrap();
    assert_eq!(
        usage,
        if duplicate {
            (1, 1, None, None, true)
        } else {
            (1, 1, Some(7), Some(3), false)
        }
    );
    let originals:(Option<i64>,Option<i64>)=sqlx::query_as("SELECT response_original_input_tokens,response_original_output_tokens FROM model_route_advisory_attempts WHERE id=$1").bind(attempt).fetch_one(&pool).await.unwrap();
    assert_eq!(originals, (None, None));
    let matrix_calls: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        matrix_calls, 0,
        "fixture selected a no-call Matrix opportunity"
    );
    owner.finish().await;
    daemon.kill().await.unwrap();
    daemon.wait().await.unwrap();
}
