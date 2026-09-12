//! Real Git + application + non-owner PostgreSQL source/selection contracts.
use sqlx::{PgPool, Row};
use std::{path::Path, process::Command, sync::Arc};
use tect_application::WorkspaceService;
use tect_domain::{Error, HostAuth, RequestContext, WorkspaceState};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

fn git(path: &Path, args: &[&str]) {
    let result = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(result.status.success(), "git fixture command failed");
}
fn repository(path: &Path) {
    std::fs::create_dir(path).unwrap();
    git(path, &["init", "--quiet", "--initial-branch=main"]);
    git(
        path,
        &[
            "-c",
            "user.name=Tect Test",
            "-c",
            "user.email=tect@example.invalid",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "fixture",
        ],
    );
}
fn context(auth: &HostAuth, key: &str) -> RequestContext {
    RequestContext {
        auth: auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: key.into(),
    }
}
fn ids(state: &WorkspaceState) -> Vec<Uuid> {
    let mut ids: Vec<_> = state.selected_worktrees.iter().map(|w| w.id).collect();
    ids.sort();
    ids
}
async fn versions(pool: &PgPool, tenant: Uuid) -> Vec<Vec<String>> {
    let mut result = Vec::new();
    for table in [
        "source_repositories",
        "source_worktrees",
        "session_worktrees",
        "workspaces",
        "agent_sessions",
        "workspace_events",
    ] {
        result.push(sqlx::query_scalar::<_, String>(&format!(
            "SELECT xmin::text || ':' || row_to_json(t)::text FROM {table} t WHERE tenant_id=$1 ORDER BY row_to_json(t)::text"
        )).bind(tenant).fetch_all(pool).await.unwrap());
    }
    result
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_sources_are_scoped_and_selections_replace_atomically() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let checkout = root.join("source-a");
    repository(&checkout);
    let linked = root.join("linked-a");
    git(
        &checkout,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    let checkout_b = root.join("source-b");
    repository(&checkout_b);
    let roots = vec![root.to_str().unwrap().to_string()];
    let enrollment = admin::enroll_host(&pool, None, roots.clone())
        .await
        .unwrap();
    let tenant = enrollment.tenant_id;
    let store = Arc::new(PgStore::connect(&runtime_url, 12).await.unwrap());
    let service = Arc::new(WorkspaceService::new(
        store.clone(),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let ctx = context(&enrollment.auth, "logical-source-workspace");
    assert_eq!(
        service
            .register_source(&ctx, checkout.to_str().unwrap())
            .await,
        Err(Error::WorkspaceNotOpen)
    );
    let opened = service.open_workspace(&ctx).await.unwrap();
    assert!(opened.selected_worktrees.is_empty());
    let a = service
        .register_source(&ctx, checkout.to_str().unwrap())
        .await
        .unwrap();
    let a_linked = service
        .register_source(&ctx, linked.to_str().unwrap())
        .await
        .unwrap();
    let b = service
        .register_source(&ctx, checkout_b.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(a.repository_id, a_linked.repository_id);
    assert_ne!(a.id, a_linked.id);
    assert_ne!(a.repository_id, b.repository_id);
    assert_eq!(a.path, checkout.to_str().unwrap());
    let before = versions(&pool, tenant).await;
    assert_eq!(
        service
            .register_source(&ctx, linked.to_str().unwrap())
            .await
            .unwrap(),
        a_linked
    );
    assert_eq!(versions(&pool, tenant).await, before);

    // Concurrent registration by different native sessions converges to one identity.
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let (svc, peer, path) = (
            service.clone(),
            context(&enrollment.auth, &ctx.workspace_key),
            checkout.clone(),
        );
        tasks.push(tokio::spawn(async move {
            svc.open_workspace(&peer).await.unwrap();
            svc.register_source(&peer, path.to_str().unwrap())
                .await
                .unwrap()
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), a);
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM source_repositories WHERE tenant_id=$1")
            .bind(tenant)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM source_worktrees WHERE tenant_id=$1")
        .bind(tenant)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 3);

    let before = versions(&pool, tenant).await;
    let outside = tempfile::tempdir().unwrap();
    let external = outside.path().canonicalize().unwrap().join("outside-repo");
    repository(&external);
    let escape = root.join("escape");
    std::os::unix::fs::symlink(&external, &escape).unwrap();
    for invalid in [root.clone(), root.join("absent"), external, escape] {
        assert_eq!(
            service
                .register_source(&ctx, invalid.to_str().unwrap())
                .await,
            Err(Error::InvalidSource)
        );
    }
    assert_eq!(
        service.register_source(&ctx, &"x".repeat(4097)).await,
        Err(Error::InvalidSource)
    );
    assert_eq!(versions(&pool, tenant).await, before);

    let cleared = service.select_worktrees(&ctx, &[]).await.unwrap();
    assert!(cleared.selected_worktrees.is_empty());
    assert_eq!(
        ids(&service.select_worktrees(&ctx, &[a.id]).await.unwrap()),
        [a.id]
    );
    let selected = service.select_worktrees(&ctx, &[a.id, b.id]).await.unwrap();
    let mut expected = vec![a.id, b.id];
    expected.sort();
    assert_eq!(ids(&selected), expected);
    let before = versions(&pool, tenant).await;
    for invalid in [
        vec![a.id, a.id],
        vec![a.id, Uuid::new_v4()],
        vec![Uuid::nil()],
        (0..101).map(|_| Uuid::new_v4()).collect(),
    ] {
        assert_eq!(
            service.select_worktrees(&ctx, &invalid).await,
            Err(Error::InvalidWorktreeSelection)
        );
        assert_eq!(service.get_state(&ctx).await.unwrap(), selected);
    }
    assert_eq!(versions(&pool, tenant).await, before);

    let peer = context(&enrollment.auth, &ctx.workspace_key);
    let peer_open = service.open_workspace(&peer).await.unwrap();
    assert_eq!(peer_open.workspace, selected.workspace);
    assert!(peer_open.selected_worktrees.is_empty());
    service
        .select_worktrees(&peer, &[a_linked.id])
        .await
        .unwrap();
    assert_eq!(service.get_state(&ctx).await.unwrap(), selected);
    assert_eq!(ids(&service.get_state(&peer).await.unwrap()), [a_linked.id]);

    // UUID catalog pages are bounded, complete and do not mutate any logical rows.
    let before = versions(&pool, tenant).await;
    let mut seen = Vec::new();
    let mut after = None;
    loop {
        let page = service.list_sources(&ctx, after, 1).await.unwrap();
        assert!(page.items.len() <= 1);
        seen.extend(page.items.into_iter().map(|s| s.id));
        if page.next_after.is_none() {
            break;
        }
        after = page.next_after;
    }
    let mut catalog = vec![a.id, a_linked.id, b.id];
    catalog.sort();
    assert_eq!(seen, catalog);
    for limit in [0, 101, u32::MAX] {
        assert_eq!(
            service.list_sources(&ctx, None, limit).await,
            Err(Error::InvalidArguments)
        );
    }
    assert_eq!(service.open_workspace(&ctx).await.unwrap(), selected);
    assert_eq!(versions(&pool, tenant).await, before);

    // State is DB-only, even when a previously registered source becomes unavailable.
    let moved = root.join("temporarily-unavailable");
    std::fs::rename(&checkout_b, &moved).unwrap();
    assert_eq!(service.get_state(&ctx).await.unwrap(), selected);
    assert_eq!(service.open_workspace(&ctx).await.unwrap(), selected);
    std::fs::rename(&moved, &checkout_b).unwrap();

    let foreign_workspace = context(&enrollment.auth, "other-workspace");
    let sibling = admin::enroll_host(&pool, Some(tenant), roots.clone())
        .await
        .unwrap();
    let sibling_ctx = context(&sibling.auth, &ctx.workspace_key);
    let outsider = admin::enroll_host(&pool, None, roots).await.unwrap();
    let outsider_ctx = context(&outsider.auth, &ctx.workspace_key);
    let mut foreign_sources = Vec::new();
    for foreign in [&foreign_workspace, &sibling_ctx, &outsider_ctx] {
        service.open_workspace(foreign).await.unwrap();
        let source = service
            .register_source(foreign, checkout.to_str().unwrap())
            .await
            .unwrap();
        assert_ne!(source.id, a.id);
        assert_ne!(source.repository_id, a.repository_id);
        foreign_sources.push(source.id);
    }
    let before = versions(&pool, tenant).await;
    for foreign in &foreign_sources {
        assert_eq!(
            service.select_worktrees(&ctx, &[a.id, *foreign]).await,
            Err(Error::InvalidWorktreeSelection)
        );
    }
    assert_eq!(versions(&pool, tenant).await, before);
    assert_eq!(service.get_state(&ctx).await.unwrap(), selected);
    assert_eq!(
        service
            .list_sources(&ctx, None, 100)
            .await
            .unwrap()
            .items
            .len(),
        3
    );

    // Direct SQL under runtime RLS cannot link a session to another workspace/host/tenant.
    for foreign in foreign_sources {
        let mut tx = store.pool().begin().await.unwrap();
        sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let result = sqlx::query("INSERT INTO session_worktrees (tenant_id,workspace_id,host_id,session_id,worktree_id) VALUES ($1,$2,$3,$4,$5)")
            .bind(tenant).bind(opened.workspace.as_ref().unwrap().id).bind(ctx.auth.host_id).bind(opened.session.as_ref().unwrap().id).bind(foreign).execute(&mut *tx).await;
        assert!(result.is_err());
        tx.rollback().await.unwrap();
    }
    for table in [
        "source_repositories",
        "source_worktrees",
        "session_worktrees",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        let row = sqlx::query(
            "SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE oid=$1::regclass",
        )
        .bind(table)
        .fetch_one(store.pool())
        .await
        .unwrap();
        assert!(row.get::<bool, _>("relrowsecurity") && row.get::<bool, _>("relforcerowsecurity"));
    }
    assert!(
        service
            .select_worktrees(&ctx, &[])
            .await
            .unwrap()
            .selected_worktrees
            .is_empty()
    );
    assert_eq!(ids(&service.get_state(&peer).await.unwrap()), [a_linked.id]);
}
