//! Preparation must finish while workspace opening still owns its transaction.
use sqlx::PgPool;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tect_application::WorkspaceService;
use tect_domain::{Error, RequestContext};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

async fn versions(pool: &PgPool, tenant: Uuid) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    for table in [
        "workspaces",
        "memberships",
        "agent_sessions",
        "workspace_events",
    ] {
        result.push(
            sqlx::query_scalar::<_, String>(&format!(
                "SELECT xmin::text || ':' || row_to_json(t)::text FROM {table} t \
                 WHERE tenant_id=$1 ORDER BY row_to_json(t)::text"
            ))
            .bind(tenant)
            .fetch_all(pool)
            .await
            .unwrap(),
        );
    }
    result
}

#[tokio::test]
async fn preparation_failure_rolls_back_new_open_and_preserves_existing_open() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let host = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let service = WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    );
    let context = RequestContext {
        auth: host.auth,
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: "prepared-open-rollback".into(),
    };
    let calls = AtomicUsize::new(0);
    let before = versions(&pool, host.tenant_id).await;
    assert!(before.iter().all(Vec::is_empty));
    let failed = service
        .open_workspace_prepared(&context, |state| {
            calls.fetch_add(1, Ordering::SeqCst);
            assert!(state.workspace.is_some());
            assert!(state.session.is_some());
            Err::<(), _>(Error::RequestTooLarge)
        })
        .await;
    assert_eq!(failed, Err(Error::RequestTooLarge));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(versions(&pool, host.tenant_id).await, before);

    // The allocation identity proves the exact prepared object is returned.
    let prepared = Box::new("prepared-result".to_string());
    let pointer = (&*prepared) as *const String;
    let (opened, returned) = service
        .open_workspace_prepared(&context, |state| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok((state, prepared))
        })
        .await
        .unwrap();
    assert_eq!((&*returned) as *const String, pointer);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(service.get_state(&context).await.unwrap(), opened);
    let persisted = versions(&pool, host.tenant_id).await;
    assert_eq!(
        persisted.iter().map(Vec::len).collect::<Vec<_>>(),
        [1, 1, 1, 2]
    );

    assert_eq!(
        service
            .open_workspace_prepared(&context, |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<(), _>(Error::RequestTooLarge)
            })
            .await,
        Err(Error::RequestTooLarge)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(versions(&pool, host.tenant_id).await, persisted);

    let recovered = service
        .open_workspace_prepared(&context, |state| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok((state, "existing-prepared-result"))
        })
        .await
        .unwrap();
    assert_eq!(recovered, (opened.clone(), "existing-prepared-result"));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(service.open_workspace(&context).await.unwrap(), opened);
    assert_eq!(versions(&pool, host.tenant_id).await, persisted);
}
