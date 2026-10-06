use super::*;
use tect_application::AntiBloatAttemptState;
use tect_host::{JevAntiBloatConfig, JevAntiBloatProvider};
pub(super) async fn run(
    admin_pool: &sqlx::PgPool,
    runtime_pool: &sqlx::PgPool,
    store: &PgStore,
    enrollment: &crate::admin::Enrollment,
    tenant: Uuid,
    actor: Uuid,
    workspace: Uuid,
    session: Uuid,
    candidate: Uuid,
) {
    let (policy, keys) = super::native_support::signed_scope_budget_fixture(workspace, actor, 1);
    let trusted = store.clone().with_budget_owner_keys(keys);
    let mut tx = rw(&trusted, &enrollment.auth, tenant).await;
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(workspace, &policy)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let ((endpoint, done, server), release) = super::native_transport::held().await;
    let provider = JevAntiBloatProvider::new(
        JevAntiBloatConfig {
            profile: "fixture".into(),
            endpoint,
            model: "jev".into(),
            timeout: std::time::Duration::from_secs(2),
            maximum_request_bytes: 65536,
            maximum_response_bytes: 16384,
        },
        "fake-local-key".into(),
    )
    .unwrap();
    let adapters = Arc::new(UnusedVerifierAdapters);
    let service = WorkspaceService::new(Arc::new(trusted), adapters.clone(), adapters)
        .with_anti_bloat_provider(Arc::new(provider));
    let context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: session.to_string(),
        workspace_key: format!("anti-bloat-live-{workspace}"),
    };
    let first = service
        .prepare_anti_bloat(
            &context,
            candidate,
            4,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await
        .unwrap();
    let second = service
        .prepare_anti_bloat(
            &context,
            candidate,
            4,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await
        .unwrap();
    assert_eq!(first.state, AntiBloatAttemptState::Prepared);
    assert_eq!(second.state, AntiBloatAttemptState::Prepared);
    let before: (i64, i64, i64, i64, i64, i64, i64, i64) =
        sqlx::query_as("SELECT * FROM advisory_budget_policy_usage_totals($1,$2,$3,$4,$5)")
            .bind(tenant)
            .bind(workspace)
            .bind(policy.id())
            .bind(policy.version())
            .bind(policy.digest())
            .fetch_one(admin_pool)
            .await
            .unwrap();
    assert_eq!(before, (0, 0, 0, 0, 0, 0, 0, 0));
    let mut first_run = Box::pin(service.run_anti_bloat_once(&context, first.review_id));
    let mut second_run = Box::pin(service.run_anti_bloat_once(&context, second.review_id));
    let (first_lost, denial) =
        tokio::select! {v=&mut first_run=>(true,v),v=&mut second_run=>(false,v)};
    assert_eq!(
        denial,
        Err(Error::BudgetPolicyInvalid),
        "pending envelope blocks sibling before response"
    );
    let pending: (i64, i64, i64, i64, i64, i64, i64, i64) =
        sqlx::query_as("SELECT * FROM advisory_budget_policy_usage_totals($1,$2,$3,$4,$5)")
            .bind(tenant)
            .bind(workspace)
            .bind(policy.id())
            .bind(policy.version())
            .bind(policy.digest())
            .fetch_one(admin_pool)
            .await
            .unwrap();
    assert_eq!(
        (
            pending.0, pending.2, pending.3, pending.4, pending.6, pending.7
        ),
        (1, 0, 0, 0, 1, 0)
    );
    release.send(()).unwrap();
    let (winner, loser, original) = if first_lost {
        (second.review_id, first.review_id, second_run.await.unwrap())
    } else {
        (first.review_id, second.review_id, first_run.await.unwrap())
    };
    assert!(matches!(original, AntiBloatAttemptState::Ranked(_)));
    assert_eq!(
        service.run_anti_bloat_once(&context, loser).await,
        Err(Error::BudgetExhaustedBeforeDispatch)
    );
    assert_eq!(
        service.run_anti_bloat_once(&context, winner).await.unwrap(),
        original
    );
    done.send(()).unwrap();
    let (request, response, duplicate) = server.await.unwrap();
    assert!(!duplicate);
    let totals: (i64, i64, i64, i64, i64, i64, i64, i64) =
        sqlx::query_as("SELECT * FROM advisory_budget_policy_usage_totals($1,$2,$3,$4,$5)")
            .bind(tenant)
            .bind(workspace)
            .bind(policy.id())
            .bind(policy.version())
            .bind(policy.digest())
            .fetch_one(admin_pool)
            .await
            .unwrap();
    assert_eq!(
        (totals.0, totals.2, totals.3, totals.4, totals.6, totals.7),
        (1, 0, 20, 30, 0, 0)
    );
    assert_eq!(totals.1, request.len() as i64);
    assert!(totals.5 > 0);
    assert!(totals.5 < policy.ceilings().elapsed_monotonic_ms);
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM scope_anti_bloat_budget_reservations WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM scope_anti_bloat_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND workspace_id=$2 AND raw_response IS NOT NULL)").bind(tenant).bind(workspace).fetch_one(admin_pool).await.unwrap();
    assert_eq!(counts, (1, 1, 1));
    let persisted:(Vec<u8>,Vec<u8>)=sqlx::query_as("SELECT request_bytes,raw_response FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND review_id=$2").bind(tenant).bind(winner).fetch_one(admin_pool).await.unwrap();
    assert_eq!(persisted, (request.clone(), response));
    println!(
        "valid synthetic anti/anti race workspace={workspace} policy={} winner={winner} calls={} bytes={} usage={}/{} pending={} invalid={} reservations/consumptions/raw=1/1/1 provider_POSTs=1 crosspurpose=UNEXECUTED",
        policy.id(),
        totals.0,
        totals.1,
        totals.3,
        totals.4,
        totals.6,
        totals.7
    );
    runtime_pool.close().await;
    admin_pool.close().await;
}
