//! Application-boundary proof that DB-only setup paths never enter SetupFiles.

use serde_json::json;
use sqlx::PgPool;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tect_application::{SetupFiles, SetupOutputGuard, WorkspaceService};
use tect_domain::{
    Error, FileObservation, FilePublication, HostAuth, RequestContext, Result, SaveSetup, Setup,
    SetupDirectory,
};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

#[derive(Default)]
struct DenyingSetupFiles {
    calls: AtomicUsize,
}

impl DenyingSetupFiles {
    fn assert_unused(&self) {
        assert_eq!(
            self.calls.load(Ordering::SeqCst),
            0,
            "a DB-only or pre-adapter refusal entered SetupFiles"
        );
    }

    fn entered(&self) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(Error::SetupUnavailable)
    }
}

impl SetupFiles for DenyingSetupFiles {
    fn resolve_directory(&self, _: &str, _: &[String]) -> Result<SetupDirectory> {
        self.entered()?;
        unreachable!()
    }

    fn inspect(&self, _: &SetupDirectory, _: usize) -> Result<FileObservation> {
        self.entered()?;
        unreachable!()
    }

    fn publish(&self, _: &SetupDirectory, _: &str) -> Result<FilePublication> {
        self.entered()?;
        unreachable!()
    }
}

struct FixtureGuard;

impl SetupOutputGuard for FixtureGuard {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        i64::try_from(input.len()).map_err(|_| Error::RequestTooLarge)
    }

    fn check(&self, _: &Setup) -> Result<()> {
        Ok(())
    }
}

fn context(auth: &HostAuth, workspace: &str) -> RequestContext {
    RequestContext {
        auth: auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: workspace.to_owned(),
    }
}

async fn bind_and_begin(
    service: &WorkspaceService,
    context: &RequestContext,
    directory: &Path,
    narrative: &str,
) -> Setup {
    service.open_workspace(context).await.unwrap();
    let observed = service
        .inspect_setup(context, Some(directory.to_str().unwrap()), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(
        observed.file,
        FileObservation::missing(),
        "fixture directory must start without AGENTS.md"
    );
    service
        .begin_setup(context, Uuid::new_v4(), narrative, &FixtureGuard)
        .await
        .unwrap()
}

async fn setup_row(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar(
        "SELECT xmin::text || ':' || row_to_json(s)::text FROM workspace_setups s WHERE id=$1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn db_only_and_identity_refusals_make_zero_setup_files_calls() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let store = Arc::new(PgStore::connect(&runtime_url, 8).await.unwrap());
    let real = WorkspaceService::new(
        store.clone(),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    );
    let files = Arc::new(DenyingSetupFiles::default());
    let probe = WorkspaceService::new(
        store,
        Arc::new(tect_host::GitSourceInspector),
        files.clone(),
    );

    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let directory_a = root.join("task-a");
    let directory_b = root.join("task-b");
    std::fs::create_dir(&directory_a).unwrap();
    std::fs::create_dir(&directory_b).unwrap();
    let root_text = root.to_str().unwrap().to_owned();
    let host = admin::enroll_host_with_grants(&pool, None, Vec::new(), vec![root_text.clone()])
        .await
        .unwrap();
    let workspace = format!("setup-db-only-{}", Uuid::new_v4());

    // Workspace state and an inspection without launch-directory context are DB-only.
    let unopened = context(&host.auth, &workspace);
    assert_eq!(probe.get_state(&unopened).await.unwrap().workspace, None);
    probe.open_workspace(&unopened).await.unwrap();
    probe.get_state(&unopened).await.unwrap();
    let context_unknown = probe
        .inspect_setup(&unopened, None, 64 * 1024)
        .await
        .unwrap();
    assert_eq!(context_unknown.file, FileObservation::unknown());
    files.assert_unused();

    // A current authentication failure precedes path parsing or adapter entry.
    let mut bad_auth = context(&host.auth, &workspace);
    bad_auth.auth.credential = "0".repeat(64);
    assert!(matches!(
        probe
            .inspect_setup(&bad_auth, Some(directory_a.to_str().unwrap()), 64 * 1024)
            .await,
        Err(Error::Unauthorized)
    ));
    files.assert_unused();

    // A host without a matching setup-root grant gets an observation, not an adapter call.
    let no_grant = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let no_grant_context = context(&no_grant.auth, &format!("no-grant-{}", Uuid::new_v4()));
    probe.open_workspace(&no_grant_context).await.unwrap();
    let denied = probe
        .inspect_setup(
            &no_grant_context,
            Some(directory_a.to_str().unwrap()),
            64 * 1024,
        )
        .await
        .unwrap();
    assert_eq!(
        denied.file.status,
        tect_domain::SetupFileStatus::Unavailable
    );
    assert_eq!(
        denied.file.reason.as_deref(),
        Some("setup_root_not_granted")
    );
    files.assert_unused();

    // A current session with no saved directory binding refuses every setup mutation/read first.
    let unbound = context(&host.auth, &workspace);
    probe.open_workspace(&unbound).await.unwrap();
    let arbitrary = Uuid::new_v4();
    let save: SaveSetup = serde_json::from_value(json!({
        "setup_id":arbitrary,"revision":1,"input_cursor":0,"ready":false
    }))
    .unwrap();
    assert!(matches!(
        probe
            .begin_setup(&unbound, Uuid::new_v4(), "unbound", &FixtureGuard)
            .await,
        Err(Error::TaskDirectoryUnbound)
    ));
    assert!(matches!(
        probe
            .get_setup(&unbound, arbitrary, Some(0), 25, 64 * 1024)
            .await,
        Err(Error::TaskDirectoryUnbound)
    ));
    assert!(matches!(
        probe.save_setup(&unbound, &save, &FixtureGuard).await,
        Err(Error::TaskDirectoryUnbound)
    ));
    assert!(matches!(
        probe
            .record_setup_input(
                &unbound,
                arbitrary,
                1,
                Uuid::new_v4(),
                "unbound",
                &FixtureGuard,
            )
            .await,
        Err(Error::TaskDirectoryUnbound)
    ));
    assert!(matches!(
        probe
            .apply_setup(&unbound, arbitrary, 1, &FixtureGuard)
            .await,
        Err(Error::TaskDirectoryUnbound)
    ));
    files.assert_unused();

    // Real fixture bindings let the probe prove unknown and other-directory rows are DB refusals.
    let bound_a = context(&host.auth, &workspace);
    let bound_b = context(&host.auth, &workspace);
    let setup_a = bind_and_begin(&real, &bound_a, &directory_a, "protected setup A").await;
    let setup_b = bind_and_begin(&real, &bound_b, &directory_b, "protected setup B").await;
    assert!(matches!(
        probe
            .get_setup(&bound_a, Uuid::new_v4(), Some(0), 25, 64 * 1024)
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        probe
            .get_setup(&bound_a, setup_b.id, Some(0), 25, 64 * 1024)
            .await,
        Err(Error::NotFound)
    ));
    files.assert_unused();

    // Removing this fixture host's grant after binding denies all setup calls before revalidation.
    let before = setup_row(&pool, setup_a.id).await;
    sqlx::query("UPDATE hosts SET allowed_setup_roots='[]'::jsonb WHERE id=$1")
        .bind(host.auth.host_id)
        .execute(&pool)
        .await
        .unwrap();
    probe.get_state(&bound_a).await.unwrap();
    let denied_after_binding = probe
        .inspect_setup(&bound_a, Some(directory_a.to_str().unwrap()), 64 * 1024)
        .await
        .unwrap();
    assert_eq!(
        denied_after_binding.file.reason.as_deref(),
        Some("setup_root_not_granted")
    );
    let save_a: SaveSetup = serde_json::from_value(json!({
        "setup_id":setup_a.id,"revision":1,"input_cursor":1,"ready":false
    }))
    .unwrap();
    assert!(matches!(
        probe
            .begin_setup(&bound_a, Uuid::new_v4(), "denied", &FixtureGuard)
            .await,
        Err(Error::SetupUnavailable)
    ));
    assert!(matches!(
        probe
            .get_setup(&bound_a, setup_a.id, Some(0), 25, 64 * 1024)
            .await,
        Err(Error::SetupUnavailable)
    ));
    assert!(matches!(
        probe.save_setup(&bound_a, &save_a, &FixtureGuard).await,
        Err(Error::SetupUnavailable)
    ));
    assert!(matches!(
        probe
            .record_setup_input(
                &bound_a,
                setup_a.id,
                1,
                Uuid::new_v4(),
                "denied",
                &FixtureGuard,
            )
            .await,
        Err(Error::SetupUnavailable)
    ));
    assert!(matches!(
        probe
            .apply_setup(&bound_a, setup_a.id, 1, &FixtureGuard)
            .await,
        Err(Error::SetupUnavailable)
    ));
    assert_eq!(setup_row(&pool, setup_a.id).await, before);
    assert!(!directory_a.join("AGENTS.md").exists());
    files.assert_unused();

    // Restore only the host grant mutated by this fixture.
    sqlx::query("UPDATE hosts SET allowed_setup_roots=$2 WHERE id=$1")
        .bind(host.auth.host_id)
        .bind(sqlx::types::Json(vec![root_text]))
        .execute(&pool)
        .await
        .unwrap();

    // Revocation is also checked before the adapter for a path-bearing setup request.
    let revoked = admin::enroll_host_with_grants(
        &pool,
        None,
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    admin::revoke_host(&pool, revoked.auth.host_id)
        .await
        .unwrap();
    let revoked_context = context(&revoked.auth, &format!("revoked-{}", Uuid::new_v4()));
    assert!(matches!(
        probe
            .inspect_setup(
                &revoked_context,
                Some(directory_a.to_str().unwrap()),
                64 * 1024,
            )
            .await,
        Err(Error::Unauthorized)
    ));
    files.assert_unused();
}
