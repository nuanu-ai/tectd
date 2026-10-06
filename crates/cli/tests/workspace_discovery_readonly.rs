//! Continuation discovery never establishes or changes task-directory identity.
use sqlx::PgPool;
use std::{path::Path, sync::Arc};
use tect_application::WorkspaceService;
use tect_domain::{Error, FileObservation, RequestContext, SetupFileStatus};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

async fn versions(pool: &PgPool, tenant: Uuid) -> Vec<Vec<String>> {
    let mut rows = vec![];
    for table in [
        "workspaces",
        "memberships",
        "agent_sessions",
        "workspace_events",
        "setup_session_directories",
        "workspace_setups",
    ] {
        rows.push(sqlx::query_scalar::<_,String>(&format!("SELECT xmin::text || ':' || row_to_json(t)::text FROM {table} t WHERE tenant_id=$1 ORDER BY row_to_json(t)::text")).bind(tenant).fetch_all(pool).await.unwrap());
    }
    rows
}
fn text_path(path: &Path) -> &str {
    path.to_str().unwrap()
}
#[tokio::test]
async fn discovery_continuation_is_readonly_and_preserves_original_observation_rules() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let directory = root.join("task");
    std::fs::create_dir(&directory).unwrap();
    let host = admin::enroll_host_with_grants(&pool, None, vec![], vec![text_path(&root).into()])
        .await
        .unwrap();
    let service = WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    );
    let context = RequestContext {
        auth: host.auth,
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: "readonly-discovery".into(),
    };
    service.open_workspace(&context).await.unwrap();
    let before = versions(&pool, host.tenant_id).await;
    let first = service
        .inspect_setup_readonly(&context, Some(text_path(&directory)), 8192)
        .await
        .unwrap();
    assert_eq!(first.file.status, SetupFileStatus::Missing);
    assert_eq!(versions(&pool, host.tenant_id).await, before);
    assert_eq!(
        service
            .inspect_setup_readonly(&context, None, 8192)
            .await
            .unwrap()
            .file,
        FileObservation::unknown()
    );
    let unavailable = service
        .inspect_setup_readonly(&context, Some("/not-granted"), 8192)
        .await
        .unwrap();
    assert_eq!(
        unavailable.file,
        FileObservation::unavailable("setup_root_not_granted")
    );
    assert_eq!(versions(&pool, host.tenant_id).await, before);
    std::fs::write(directory.join("AGENTS.md"), "first observation").unwrap();
    let initial = service
        .inspect_setup(&context, Some(text_path(&directory)), 8192)
        .await
        .unwrap();
    let bound = versions(&pool, host.tenant_id).await;
    let repeated = service
        .inspect_setup_readonly(&context, Some(text_path(&directory)), 8192)
        .await
        .unwrap();
    assert_eq!(repeated.state, initial.state);
    assert_eq!(repeated.file, initial.file);
    assert_eq!(versions(&pool, host.tenant_id).await, bound);
    std::fs::write(directory.join("AGENTS.md"), "changed observation").unwrap();
    let changed = service
        .inspect_setup_readonly(&context, Some(text_path(&directory)), 8192)
        .await
        .unwrap();
    assert_ne!(changed.file.sha256, initial.file.sha256);
    assert_eq!(versions(&pool, host.tenant_id).await, bound);
    // A replacement at the same physical path must not redirect the saved identity.
    std::fs::rename(&directory, root.join("old-task")).unwrap();
    std::fs::create_dir(&directory).unwrap();
    assert_eq!(
        service
            .inspect_setup_readonly(&context, Some(text_path(&directory)), 8192)
            .await
            .unwrap_err(),
        Error::TaskDirectoryMismatch
    );
    assert_eq!(versions(&pool, host.tenant_id).await, bound);
}
