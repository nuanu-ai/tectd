//! Actual concurrent PostgreSQL authorization and operator CLI boundaries.
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tect_application::{Store, TransactionMode, WorkspaceService};
use tect_domain::{Error, HostAuth, RequestContext, StateStatus};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

fn tagged_url(url: &str, tag: &str) -> String {
    format!(
        "{url}{}application_name={tag}",
        if url.contains('?') { "&" } else { "?" }
    )
}
fn context(auth: &HostAuth, key: &str) -> RequestContext {
    RequestContext {
        auth: auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: key.into(),
    }
}
async fn wait_for_lock(pool: &PgPool, tag: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND state='active' AND wait_event_type='Lock')")
                .bind(tag).fetch_one(pool).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("expected real PostgreSQL lock wait");
}
async fn assert_host_denied(service: &WorkspaceService, ctx: &RequestContext) {
    assert_eq!(service.get_state(ctx).await, Err(Error::Unauthorized));
    assert_eq!(service.open_workspace(ctx).await, Err(Error::Unauthorized));
    assert_eq!(
        service.select_worktrees(ctx, &[]).await,
        Err(Error::Unauthorized)
    );
    assert_eq!(
        service.list_sources(ctx, None, 1).await,
        Err(Error::Unauthorized)
    );
    assert_eq!(
        service.register_source(ctx, "/missing").await,
        Err(Error::Unauthorized)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn operator_revocation_serializes_with_admission_and_never_reopens_identity() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let runtime_tag = format!("tect-auth-test-{}", Uuid::new_v4());
    let revoke_tag = format!("tect-revoke-test-{}", Uuid::new_v4());
    let revoker = PgPool::connect(&tagged_url(&admin_url, &revoke_tag))
        .await
        .unwrap();
    let store = Arc::new(
        PgStore::connect(&tagged_url(&runtime_url, &runtime_tag), 6)
            .await
            .unwrap(),
    );
    let service = Arc::new(WorkspaceService::new(
        store.clone(),
        Arc::new(tect_host::GitSourceInspector),
    ));
    let host = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let ctx = context(&host.auth, "admitted-before-host-revoke");
    let mut blocker = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1::text || ':' || $2,0))")
        .bind(ctx.auth.host_id)
        .bind(&ctx.native_session_id)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let admitted = {
        let (svc, ctx) = (service.clone(), ctx.clone());
        tokio::spawn(async move { svc.open_workspace(&ctx).await })
    };
    // Open has authenticated FOR SHARE and now waits for our native-identity lock.
    wait_for_lock(&pool, &runtime_tag).await;
    let host_revoke = {
        let revoker = revoker.clone();
        let id = ctx.auth.host_id;
        tokio::spawn(async move { admin::revoke_host(&revoker, id).await })
    };
    wait_for_lock(&pool, &revoke_tag).await;
    assert!(!host_revoke.is_finished());
    blocker.commit().await.unwrap();
    let opened = tokio::time::timeout(Duration::from_secs(5), admitted)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(opened.status, StateStatus::Ready);
    host_revoke.await.unwrap().unwrap();
    assert_host_denied(&service, &ctx).await;
    admin::revoke_host(&revoker, ctx.auth.host_id)
        .await
        .unwrap();
    assert_eq!(
        admin::revoke_host(&revoker, Uuid::new_v4()).await,
        Err(Error::NotFound)
    );
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_sessions WHERE tenant_id=$1")
        .bind(host.tenant_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1);

    let host = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let ctx = context(&host.auth, "session-revocation");
    let opened = service.open_workspace(&ctx).await.unwrap();
    let session_id = opened.session.as_ref().unwrap().id;
    // An admitted mutation owns this exact native identity lock until commit.
    let mut admitted = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = admitted.authenticate(&ctx.auth).await.unwrap();
    admitted.set_tenant(identity.tenant_id).await.unwrap();
    admitted
        .lock_native_session(identity.host_id, &ctx.native_session_id)
        .await
        .unwrap();
    let session_revoke = {
        let revoker = revoker.clone();
        tokio::spawn(async move { admin::revoke_session(&revoker, session_id).await })
    };
    wait_for_lock(&pool, &revoke_tag).await;
    assert!(!session_revoke.is_finished());
    admitted.commit().await.unwrap();
    session_revoke.await.unwrap().unwrap();
    assert_eq!(
        service.open_workspace(&ctx).await,
        Err(Error::SessionRevoked)
    );
    assert_eq!(service.get_state(&ctx).await, Err(Error::SessionRevoked));
    assert_eq!(
        service.select_worktrees(&ctx, &[]).await,
        Err(Error::SessionRevoked)
    );
    assert_eq!(
        service.list_sources(&ctx, None, 1).await,
        Err(Error::SessionRevoked)
    );
    admin::revoke_session(&revoker, session_id).await.unwrap();
    assert_eq!(
        admin::revoke_session(&revoker, Uuid::new_v4()).await,
        Err(Error::NotFound)
    );
    let other = context(&ctx.auth, &ctx.workspace_key);
    let peer = service.open_workspace(&other).await.unwrap();
    assert_eq!(peer.workspace, opened.workspace);
    assert_ne!(peer.session.as_ref().unwrap().id, session_id);

    // Exercise both actual CLI commands against a disposable enrollment.
    let cli_host = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let cli_ctx = context(&cli_host.auth, "operator-cli");
    let cli_session = service
        .open_workspace(&cli_ctx)
        .await
        .unwrap()
        .session
        .unwrap()
        .id;
    for (command, flag, id) in [
        ("revoke-session", "--session-id", cli_session),
        ("revoke-host", "--host-id", cli_host.auth.host_id),
    ] {
        let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_tect-admin"))
            .args([command, flag, &id.to_string()])
            .env("TECT_ADMIN_DATABASE_URL", &admin_url)
            .output()
            .await
            .unwrap();
        assert!(output.status.success(), "operator command failed");
        assert!(!String::from_utf8_lossy(&output.stdout).contains(&cli_host.auth.credential));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(&cli_host.auth.credential));
    }
    assert_host_denied(&service, &cli_ctx).await;
    let runtime_without_scope: i64 = sqlx::query_scalar("SELECT count(*) FROM agent_sessions")
        .fetch_one(store.pool())
        .await
        .unwrap();
    assert_eq!(runtime_without_scope, 0);
    let can_update: bool = sqlx::query_scalar("SELECT has_table_privilege(current_user,'agent_sessions','UPDATE') OR has_table_privilege(current_user,'hosts','UPDATE')").fetch_one(store.pool()).await.unwrap();
    assert!(!can_update);
}
