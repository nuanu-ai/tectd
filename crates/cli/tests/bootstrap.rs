//! Real PostgreSQL contracts. Set TECT_TEST_ADMIN_URL and TECT_TEST_RUNTIME_URL.
use sqlx::{PgPool, Row};
use std::sync::Arc;
use tect_application::{Store, TransactionMode, WorkspaceService};
use tect_domain::{Error, HostAuth, RequestContext, StateStatus};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

fn context(auth: &HostAuth, key: &str) -> RequestContext {
    RequestContext {
        auth: auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: key.into(),
    }
}

async fn counts(pool: &PgPool, tenant: Uuid) -> Vec<i64> {
    let mut result = Vec::new();
    for table in [
        "workspaces",
        "memberships",
        "agent_sessions",
        "workspace_events",
    ] {
        result.push(
            sqlx::query_scalar::<_, i64>(&format!(
                "SELECT count(*) FROM {table} WHERE tenant_id=$1"
            ))
            .bind(tenant)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    result
}

async fn versions(pool: &PgPool, tenant: Uuid) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    for table in [
        "workspaces",
        "memberships",
        "agent_sessions",
        "workspace_events",
    ] {
        result.push(sqlx::query_scalar::<_, String>(
            &format!("SELECT xmin::text || ':' || row_to_json(t)::text FROM {table} t WHERE tenant_id=$1 ORDER BY row_to_json(t)::text"),
        ).bind(tenant).fetch_all(pool).await.unwrap());
    }
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bootstrap_is_atomic_native_keyed_and_tenant_isolated() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let admin_pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&admin_pool, &role).await.unwrap();
    let store = Arc::new(PgStore::connect(&runtime_url, 12).await.unwrap());
    let service = Arc::new(WorkspaceService::new(
        store.clone(),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let enrollment = admin::enroll_host(&admin_pool, None, Vec::new())
        .await
        .unwrap();
    let tenant = enrollment.tenant_id;
    let ctx = context(&enrollment.auth, "logical-workspace");

    // An unopened read cannot create a workspace, session, membership or event.
    let before = versions(&admin_pool, tenant).await;
    let state = service.get_state(&ctx).await.unwrap();
    assert_eq!(state.status, StateStatus::Uninitialized);
    assert_eq!(state.next_action.as_deref(), Some("open_workspace"));
    assert!(state.selected_worktrees.is_empty());
    assert_eq!(versions(&admin_pool, tenant).await, before);
    assert_eq!(counts(&admin_pool, tenant).await, [0, 0, 0, 0]);

    let opened = service.open_workspace(&ctx).await.unwrap();
    assert_eq!(opened.status, StateStatus::Ready);
    assert_eq!(
        opened.session.as_ref().unwrap().native_session_id,
        ctx.native_session_id
    );
    let workspace_id = opened.workspace.as_ref().unwrap().id;
    assert_eq!(counts(&admin_pool, tenant).await, [1, 1, 1, 2]);
    let before = versions(&admin_pool, tenant).await;
    for _ in 0..3 {
        assert_eq!(service.get_state(&ctx).await.unwrap(), opened);
        assert_eq!(service.open_workspace(&ctx).await.unwrap(), opened);
    }
    assert_eq!(versions(&admin_pool, tenant).await, before);

    // Same native session, concurrent delivery/lost-reply retry: one result.
    let mut calls = Vec::new();
    for _ in 0..12 {
        let (service, ctx) = (service.clone(), ctx.clone());
        calls.push(tokio::spawn(
            async move { service.open_workspace(&ctx).await },
        ));
    }
    for call in calls {
        assert_eq!(call.await.unwrap().unwrap(), opened);
    }
    assert_eq!(counts(&admin_pool, tenant).await, [1, 1, 1, 2]);

    // A second native session sees no session until it explicitly opens.
    let second = context(&enrollment.auth, &ctx.workspace_key);
    let before = versions(&admin_pool, tenant).await;
    assert_eq!(
        service.get_state(&second).await.unwrap().status,
        StateStatus::Uninitialized
    );
    assert_eq!(versions(&admin_pool, tenant).await, before);
    let second_open = service.open_workspace(&second).await.unwrap();
    assert_eq!(second_open.workspace.as_ref().unwrap().id, workspace_id);
    assert_ne!(second_open.session, opened.session);
    assert_eq!(counts(&admin_pool, tenant).await, [1, 1, 2, 3]);

    // Another enrolled host can reuse a native ID; the tuple contains the host.
    let sibling = admin::enroll_host(&admin_pool, Some(tenant), Vec::new())
        .await
        .unwrap();
    let mut sibling_ctx = ctx.clone();
    sibling_ctx.auth = sibling.auth;
    let sibling_open = service.open_workspace(&sibling_ctx).await.unwrap();
    assert_eq!(sibling_open.workspace.as_ref().unwrap().id, workspace_id);
    assert_ne!(sibling_open.session, opened.session);

    let other = admin::enroll_host(&admin_pool, None, Vec::new())
        .await
        .unwrap();
    let mut other_ctx = ctx.clone();
    other_ctx.auth = other.auth.clone();
    let other_open = service.open_workspace(&other_ctx).await.unwrap();
    assert_ne!(other_open.workspace.as_ref().unwrap().id, workspace_id);

    let baseline = counts(&admin_pool, tenant).await;
    let mut spoof = ctx.clone();
    spoof.auth.credential = "0".repeat(64);
    assert_eq!(
        service.open_workspace(&spoof).await,
        Err(Error::Unauthorized)
    );
    spoof.auth = other.auth.clone();
    spoof.auth.host_id = ctx.auth.host_id;
    assert_eq!(service.get_state(&spoof).await, Err(Error::Unauthorized));
    let mut mismatch = ctx.clone();
    mismatch.workspace_key = "another-workspace".into();
    assert_eq!(
        service.open_workspace(&mismatch).await,
        Err(Error::SessionWorkspaceMismatch)
    );
    assert_eq!(
        service.get_state(&mismatch).await,
        Err(Error::SessionWorkspaceMismatch)
    );
    assert_eq!(counts(&admin_pool, tenant).await, baseline);

    // Competing first opens with the same native identity cannot orphan a workspace.
    let first_race = context(&enrollment.auth, "race-a");
    let mut second_race = first_race.clone();
    second_race.workspace_key = "race-b".into();
    let (left, right) = tokio::join!(
        service.open_workspace(&first_race),
        service.open_workspace(&second_race)
    );
    assert!(matches!(
        (&left, &right),
        (Ok(_), Err(Error::SessionWorkspaceMismatch))
            | (Err(Error::SessionWorkspaceMismatch), Ok(_))
    ));
    let after = counts(&admin_pool, tenant).await;
    assert_eq!(after[0], baseline[0] + 1);
    assert_eq!(after[2], baseline[2] + 1);
    assert_eq!(after[3], baseline[3] + 2);

    // An interrupted transaction drops all its partial rows and remains retryable.
    let mut abandoned = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = abandoned.authenticate(&enrollment.auth).await.unwrap();
    abandoned.set_tenant(identity.tenant_id).await.unwrap();
    abandoned.ensure_workspace("rollback-probe").await.unwrap();
    drop(abandoned);
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workspaces WHERE tenant_id=$1 AND key='rollback-probe')",
    )
    .bind(tenant)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert!(!exists);
    let recovered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        service.open_workspace(&context(&enrollment.auth, "rollback-probe")),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(recovered.status, StateStatus::Ready);

    // Real runtime-role RLS and connection-pool context lifetime.
    let role_state =
        sqlx::query("SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname=current_user")
            .fetch_one(store.pool())
            .await
            .unwrap();
    assert!(!role_state.get::<bool, _>("rolsuper"));
    assert!(!role_state.get::<bool, _>("rolbypassrls"));
    let without_scope: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_sessions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(without_scope, 0);
    assert!(
        sqlx::query("SELECT * FROM hosts")
            .fetch_all(store.pool())
            .await
            .is_err()
    );
    let mut scoped = store.pool().begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *scoped)
        .await
        .unwrap();
    let foreign_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces WHERE id=$1")
        .bind(other_open.workspace.as_ref().unwrap().id)
        .fetch_one(&mut *scoped)
        .await
        .unwrap();
    assert_eq!(foreign_rows, 0);
    scoped.commit().await.unwrap();
    let without_scope: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(without_scope, 0);
    let mut tx = store.begin(TransactionMode::ReadOnly).await.unwrap();
    tx.authenticate(&enrollment.auth).await.unwrap();
    assert!(tx.set_tenant(other.tenant_id).await.is_err());
    drop(tx);
    assert!(PgStore::connect(&admin_url, 1).await.is_err());

    // Force the same physical connection to alternate tenants under concurrent load.
    let reused = Arc::new(PgStore::connect(&runtime_url, 1).await.unwrap());
    let reused_service = Arc::new(WorkspaceService::new(
        reused.clone(),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let mut alternating = Vec::new();
    for index in 0..100 {
        let (svc, request, expected) = if index % 2 == 0 {
            (reused_service.clone(), ctx.clone(), workspace_id)
        } else {
            (
                reused_service.clone(),
                other_ctx.clone(),
                other_open.workspace.as_ref().unwrap().id,
            )
        };
        alternating.push(tokio::spawn(async move {
            let state = svc.get_state(&request).await.unwrap();
            assert_eq!(state.workspace.unwrap().id, expected);
        }));
    }
    for task in alternating {
        task.await.unwrap();
    }
    let leaked: Option<String> =
        sqlx::query_scalar("SELECT current_setting('tect.tenant_id',true)")
            .fetch_one(reused.pool())
            .await
            .unwrap();
    assert!(leaked.is_none_or(|value| value.is_empty()));
    let mut rollback = reused.begin(TransactionMode::ReadOnly).await.unwrap();
    rollback.authenticate(&ctx.auth).await.unwrap();
    rollback.set_tenant(tenant).await.unwrap();
    drop(rollback);
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
        .fetch_one(reused.pool())
        .await
        .unwrap();
    assert_eq!(visible, 0);
    let mut bad = ctx.clone();
    bad.auth.credential = "0".repeat(64);
    assert_eq!(
        reused_service.get_state(&bad).await,
        Err(Error::Unauthorized)
    );
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM workspaces")
        .fetch_one(reused.pool())
        .await
        .unwrap();
    assert_eq!(visible, 0);
}
