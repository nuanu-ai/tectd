//! The actual host wire path must reject an undeliverable open before committing.
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc};
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, StateStatus};
use tect_postgres::{PgStore, admin};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};
use uuid::Uuid;

async fn versions(pool: &PgPool, tenant: Uuid) -> Vec<Vec<String>> {
    let mut rows = vec![];
    for table in [
        "workspaces",
        "memberships",
        "agent_sessions",
        "workspace_events",
    ] {
        rows.push(sqlx::query_scalar::<_,String>(&format!("SELECT xmin::text || ':' || row_to_json(t)::text FROM {table} t WHERE tenant_id=$1 ORDER BY row_to_json(t)::text")).bind(tenant).fetch_all(pool).await.unwrap());
    }
    rows
}
async fn open(socket: &Path, context: &RequestContext, capacity: usize) -> Value {
    wire(socket, context, "open_workspace", json!({}), capacity).await
}
async fn wire(
    socket: &Path,
    context: &RequestContext,
    tool_name: &str,
    arguments: Value,
    capacity: usize,
) -> Value {
    let stream = UnixStream::connect(socket).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let request = json!({"api_version":2,"context":context,"tool_name":tool_name,"arguments":arguments,"output_capacity":capacity});
    let mut encoded = serde_json::to_vec(&request).unwrap();
    encoded.push(b'\n');
    writer.write_all(&encoded).await.unwrap();
    writer.shutdown().await.unwrap();
    let mut line = String::new();
    BufReader::new(reader).read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}
#[tokio::test]
async fn tiny_host_wire_capacity_rolls_back_new_open_and_preserves_existing_rows() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let host = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let service = Arc::new(WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let context = RequestContext {
        auth: host.auth,
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: "wire-capacity-rollback".into(),
    };
    let temp = tempfile::tempdir().unwrap();
    std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = temp.path().join("wire.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let task = tokio::spawn(tect_host::serve(listener, service.clone()));
    let before = versions(&pool, host.tenant_id).await;
    let failure = open(&socket, &context, 1).await;
    assert_eq!(failure["status"], "error");
    assert_eq!(failure["error"], json!(tect_domain::Error::RequestTooLarge));
    assert_eq!(versions(&pool, host.tenant_id).await, before);
    assert_eq!(
        service.get_state(&context).await.unwrap().status,
        StateStatus::Uninitialized
    );
    let success = open(&socket, &context, 8192).await;
    assert_eq!(success["status"], "ok");
    assert_eq!(success["result"]["status"], "ready");
    let persisted = versions(&pool, host.tenant_id).await;
    assert_eq!(
        persisted.iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 1, 1, 2]
    );
    for view in ["candidate_sets", "native_planning"] {
        let page = wire(
            &socket,
            &context,
            "workspace_state",
            json!({
                "view":view,"origin":"state","action_seed":Uuid::new_v4(),"limit":25
            }),
            8192,
        )
        .await;
        assert_eq!(page["status"], "ok");
        let state = &page["result"]["state"];
        assert_eq!(state["view"], view);
        assert_eq!(state[view], json!([]));
        assert_eq!(state["actions"], json!([]));
        assert_eq!(state["recommended_action"], Value::Null);
        assert_eq!(state["next_after"], Value::Null);
        assert_eq!(versions(&pool, host.tenant_id).await, persisted);
    }
    let failure = open(&socket, &context, 1).await;
    assert_eq!(failure["error"], json!(tect_domain::Error::RequestTooLarge));
    assert_eq!(versions(&pool, host.tenant_id).await, persisted);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
