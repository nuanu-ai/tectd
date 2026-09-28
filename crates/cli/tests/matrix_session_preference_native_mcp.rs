//! Native Matrix preference binding through MCP and disposable PostgreSQL 18.
//! The local provider is synthetic and cannot access a network endpoint.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use async_trait::async_trait;
use recovery_support::{Mcp, host_file, private_temp};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::{route, route_error};
use tect_application::{
    MatrixAdviceProvider, MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt, WorkspaceService,
};
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error, Result};
use tect_postgres::{PgStore, admin};
use tokio::net::UnixListener;
use uuid::Uuid;

const SYSTEM_ID: &str = "7690404534065724697";
const DATABASE: &str = "tect_matrix_session_s00c";

struct LocalProvider(Arc<AtomicUsize>);

#[async_trait]
impl MatrixAdviceProvider for LocalProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        Some(MatrixProviderIdentity {
            provider_profile_ref: AdvisoryProviderProfileRef {
                id: "synthetic-local".into(),
            },
            model_configuration: AdvisoryModelConfiguration {
                model: "synthetic".into(),
            },
            destination: "synthetic:local".into(),
            wire_version: "synthetic/1".into(),
        })
    }

    fn prepare(&self, _: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        Err(Error::TransportUnavailable)
    }

    async fn attempt_prepared(
        &self,
        _: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(Error::TransportUnavailable)
    }
}

fn input() -> Value {
    let absent = json!({"state":"absent"});
    json!({
        "mode":{"state":"known","value":"demo","provenance":"synthetic fixture"},
        "envelope":{"scale":absent,"operational_facts":{"state":"absent"}},
        "criticality":{"state":"known","value":"low","provenance":"synthetic fixture"},
        "intent":absent,"urgency":absent,"promised_behavior":absent,
        "promised_proof":absent,"affected_guarantees":absent,
        "actual_exposure":absent,"demand_commitment":absent,
        "latency_commitment":absent,"urgent_repair":absent
    })
}

async fn guarded_pool() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let actual: (String, String, i32, String) = sqlx::query_as(
        "SELECT current_database(),current_user,current_setting('server_version_num')::integer,\
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        actual,
        (DATABASE.into(), "postgres".into(), 180006, SYSTEM_ID.into())
    );
    let runtime: (String, String) = sqlx::query_as("SELECT current_database(),current_user")
        .fetch_one(&PgPool::connect(&runtime_url).await.unwrap())
        .await
        .unwrap();
    assert_eq!(runtime, (DATABASE.into(), "tect_ci".into()));
    (pool, runtime_url)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only a guarded disposable PostgreSQL 18.6 database"]
async fn native_matrix_sessions_snapshot_skip_replay_and_forgery_denial() {
    let (pool, runtime_url) = guarded_pool().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("matrix-session.sock");
    let enrolled = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_advisory_adapters(
            Arc::new(LocalProvider(calls.clone())),
            Arc::new(tect_application::DenyMatrixBudget),
        ),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let host = root.join("host.json");
    host_file(&host, &enrolled.auth);
    let key = format!("matrix-session-{}", Uuid::new_v4());
    let mut first = Mcp::start(&socket, &host, &Uuid::new_v4().to_string(), &key).await;
    let mut second = Mcp::start(&socket, &host, &Uuid::new_v4().to_string(), &key).await;
    let workspace = first.call("open_workspace", json!({})).await["workspace"]["id"].clone();
    assert_eq!(
        second.call("open_workspace", json!({})).await["workspace"]["id"],
        workspace
    );
    let task = Uuid::new_v4();
    route(
        &mut first,
        "command",
        "task.source.record",
        json!({
            "task_id":task,"revision":1,"expected_current_revision":0,
            "request_id":Uuid::new_v4(),"input":input()
        }),
    )
    .await;

    // Disabled wins even when the session has committed Skip.
    let set = route(
        &mut first,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":0,"preference":"skip"}),
    )
    .await;
    assert_eq!(set["revision"], 1);
    let disabled = route(
        &mut first,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,"request_key":"disabled"
        }),
    )
    .await;
    assert_eq!(disabled["reason"], "workspace_disabled");

    route(
        &mut first,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":"synthetic-local"},
            "model_configuration":{"model":"synthetic"}
        }),
    )
    .await;
    let key = "session-skip";
    let args = json!({"task_id":task,"expected_task_revision":1,"request_key":key});
    let skipped = route(
        &mut first,
        "command",
        "engineering.advisory.request",
        args.clone(),
    )
    .await;
    assert_eq!(skipped["reason"], "session_skip", "{skipped}");
    assert_eq!(skipped["state"], "no_call");
    let saved: (String, String, i64) = sqlx::query_as(
        "SELECT session_preference,request_preference,\
         (SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=o.id)\
         FROM advisory_opportunity o WHERE id=$1",
    )
    .bind(Uuid::parse_str(skipped["opportunity_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(saved, ("skip".into(), "use_workspace".into(), 0));
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let forged = route_error(
        &mut second,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,"request_key":"forged",
            "session_preference":"skip"
        }),
    )
    .await;
    assert_eq!(forged["error"]["code"], "invalid_arguments");
    let other_session = route_error(
        &mut second,
        "command",
        "engineering.advisory.request",
        args.clone(),
    )
    .await;
    assert_eq!(other_session["error"]["code"], "input_conflict");
    let changed_request = route_error(
        &mut first,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,"request_key":key,"request_preference":"skip"
        }),
    )
    .await;
    assert_eq!(changed_request["error"]["code"], "input_conflict");

    route(
        &mut first,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":1,"preference":"use_workspace"}),
    )
    .await;
    let replay = route(&mut first, "command", "engineering.advisory.request", args).await;
    assert_eq!(replay, skipped);
    let second_request = route(
        &mut second,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,"request_key":"other-session"
        }),
    )
    .await;
    assert_eq!(second_request["reason"], "choice_set_not_applicable");
    let request_skip = route(
        &mut second,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,
            "request_key":"request-skip","request_preference":"skip"
        }),
    )
    .await;
    assert_eq!(request_skip["reason"], "request_skip");
    let request_snapshot: (String, String) = sqlx::query_as(
        "SELECT session_preference,request_preference FROM advisory_opportunity WHERE id=$1",
    )
    .bind(Uuid::parse_str(request_skip["opportunity_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(request_snapshot, ("use_workspace".into(), "skip".into()));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    first.finish().await;
    second.finish().await;
    server.abort();
}
