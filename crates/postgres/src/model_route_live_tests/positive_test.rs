use super::*;
use crate::PgStore;
use crate::model_route_live_tests::positive::fixture;
use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tect_application::{
    DecideModelRouteRecommendation, DispositionModelRouteRecommendation, ModelRouteAttemptStore,
    ModelRouteCatalogueProvider, ModelRouteDecisionInput, ModelRouteDecisionOutcome,
    ModelRouteDispositionAction, ModelRouteHostCapabilitiesProvider, ModelRouteInvocation,
    ModelRoutePreparation, ModelRoutePreparedAttempt, ModelRouteProviderObservation,
    ModelRouteRankingProvider, ModelRouteSelectionRead, ModelRouteSendPermit, ModelRouteSendStart,
    PrepareModelRouteRecommendation, Store, TransactionMode, attempt_model_route_after_commit,
    attempt_model_route_observed_after_commit, finalize_model_route_sealed_response,
    prepare_model_route_send, seal_model_route_raw_response,
};
use tect_domain::{
    AdvisoryBudgetCeilings, AdvisoryBudgetPolicy, AdvisoryRequestPreference,
    MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA,
    MODEL_ROUTE_RANKING_WIRE_SCHEMA, ModelRoute, ModelRouteCatalogue, ModelRouteFactProvenance,
    ModelRouteHostCapabilities, ModelRouteRankingWireRequest,
};

mod negative_cases;
mod service_test;

/// Explicit test-only trust boundary for synthetic legacy policies. Production
/// PgUnitOfWork requires a pinned owner key and a valid signature.
struct TrustedTestRouteStore<'a> {
    inner: &'a mut dyn ModelRouteAttemptStore,
    policy: AdvisoryBudgetPolicy,
    workspace: Uuid,
}

impl<'a> TrustedTestRouteStore<'a> {
    fn new(
        inner: &'a mut dyn ModelRouteAttemptStore,
        policy: &AdvisoryBudgetPolicy,
        workspace: Uuid,
    ) -> Self {
        Self {
            inner,
            policy: policy.clone(),
            workspace,
        }
    }
}

#[async_trait]
impl ModelRouteAttemptStore for TrustedTestRouteStore<'_> {
    async fn authorized_budget_policy(
        &mut self,
        workspace: Uuid,
        now: i64,
    ) -> tect_domain::Result<Option<AdvisoryBudgetPolicy>> {
        Ok(
            (workspace == self.workspace && self.policy.is_effective_at(now))
                .then(|| self.policy.clone()),
        )
    }
    async fn by_preparation(
        &mut self,
        workspace: Uuid,
        key: &str,
        invocation: ModelRouteInvocation,
    ) -> tect_domain::Result<Option<tect_application::ModelRouteAttemptSnapshot>> {
        self.inner.by_preparation(workspace, key, invocation).await
    }
    async fn recover_raw_sealed(
        &mut self,
        workspace: Uuid,
        key: &str,
        invocation: ModelRouteInvocation,
    ) -> tect_domain::Result<Option<(ModelRoutePreparedAttempt, ModelRouteSendPermit)>> {
        self.inner
            .recover_raw_sealed(workspace, key, invocation)
            .await
    }
    async fn record_no_call(
        &mut self,
        prepared: &tect_application::PreparedModelRouteRecommendation,
        invocation: ModelRouteInvocation,
        reason: tect_application::ModelRouteRunNoCall,
    ) -> tect_domain::Result<()> {
        self.inner
            .record_no_call(prepared, invocation, reason)
            .await
    }
    async fn begin_send(
        &mut self,
        prepared: &tect_application::PreparedModelRouteRecommendation,
        invocation: ModelRouteInvocation,
        attempted: &ModelRoutePreparedAttempt,
        policy: &AdvisoryBudgetPolicy,
    ) -> tect_domain::Result<Option<ModelRouteSendPermit>> {
        self.inner
            .begin_send(prepared, invocation, attempted, policy)
            .await
    }
    async fn seal_raw_response(
        &mut self,
        permit: &ModelRouteSendPermit,
        raw: &[u8],
        digest: &str,
    ) -> tect_domain::Result<()> {
        self.inner.seal_raw_response(permit, raw, digest).await
    }
    async fn sealed_response(
        &mut self,
        permit: &ModelRouteSendPermit,
    ) -> tect_domain::Result<Option<Vec<u8>>> {
        self.inner.sealed_response(permit).await
    }
    async fn consume_budget(
        &mut self,
        permit: &ModelRouteSendPermit,
        observation: &ModelRouteProviderObservation,
    ) -> tect_domain::Result<bool> {
        self.inner.consume_budget(permit, observation).await
    }
    async fn consumption_healthy(
        &mut self,
        permit: &ModelRouteSendPermit,
    ) -> tect_domain::Result<Option<bool>> {
        self.inner.consumption_healthy(permit).await
    }
    async fn capture_sealed_outcome(
        &mut self,
        evidence: &tect_application::ModelRouteSealedRankingEvidence,
    ) -> tect_domain::Result<()> {
        self.inner.capture_sealed_outcome(evidence).await
    }
    async fn mark_send_unknown(
        &mut self,
        permit: &ModelRouteSendPermit,
    ) -> tect_domain::Result<()> {
        self.inner.mark_send_unknown(permit).await
    }
}

async fn install_synthetic_policy(
    store: &PgStore,
    workspace: Uuid,
    tenant: Uuid,
    owner: &crate::admin::Enrollment,
) -> AdvisoryBudgetPolicy {
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let id = Uuid::new_v4();
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 2,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 100_000,
        elapsed_monotonic_ms: 30_000,
        retry_dispatches: 1,
    };
    let from = now - 60_000;
    let until = now + 600_000;
    let policy = AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, from, until, ceilings),
        from,
        until,
        ceilings,
        owner.principal_id,
        "a".repeat(128),
    )
    .unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&owner.auth).await.unwrap();
    tx.set_tenant(tenant).await.unwrap();
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(workspace, &policy)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    policy
}

struct FakeJevRanker {
    pool: PgPool,
    tenant: Uuid,
    calls: Arc<AtomicUsize>,
    malformed: bool,
}

#[async_trait]
impl ModelRouteRankingProvider for FakeJevRanker {
    fn prepare(
        &self,
        saved: &tect_application::PreparedModelRouteRecommendation,
    ) -> tect_domain::Result<ModelRoutePreparedAttempt> {
        ModelRoutePreparedAttempt::new(ModelRouteRankingWireRequest::new(
            saved.workspace_id,
            &saved.request_key,
            &saved.work,
            saved.catalogue.as_ref().ok_or(Error::InputConflict)?,
            saved.eligible.as_ref().ok_or(Error::InputConflict)?,
            "fake-jev-ranker",
        )?)
    }

    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> tect_domain::Result<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut tx = self.pool.begin().await.map_err(crate::storage_error)?;
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(self.tenant.to_string())
            .execute(&mut *tx)
            .await
            .map_err(crate::storage_error)?;
        let row = sqlx::query(
            "SELECT state,request_payload,request_sha256 FROM model_route_advisory_attempts \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(self.tenant)
        .bind(permit.workspace_id)
        .bind(permit.attempt_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(crate::storage_error)?;
        assert_eq!(row.try_get::<String, _>("state").unwrap(), "send_unknown");
        assert_eq!(
            row.try_get::<Vec<u8>, _>("request_payload").unwrap(),
            attempted.request_bytes
        );
        assert_eq!(
            row.try_get::<String, _>("request_sha256").unwrap(),
            attempted.request_sha256
        );
        tx.rollback().await.map_err(crate::storage_error)?;
        if self.malformed {
            return Ok(b"{malformed".to_vec());
        }
        Ok(serde_json::json!({
            "schema": MODEL_ROUTE_RANKING_WIRE_SCHEMA,
            "binding_digest": attempted.request.binding_digest,
            "adviser_model": "fake-jev-ranker",
            "outcome": {"kind":"ranked","route_ids":["route-a"]}
        })
        .to_string()
        .into_bytes())
    }

    async fn attempt_prepared_observed(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> tect_domain::Result<ModelRouteProviderObservation> {
        Ok(ModelRouteProviderObservation {
            raw: self.attempt_prepared(attempted, permit).await?,
            input_tokens: Some(4),
            output_tokens: Some(3),
            elapsed_monotonic_ms: Some(1),
        })
    }
}

struct TestCatalogue;
impl ModelRouteCatalogueProvider for TestCatalogue {
    fn catalogue(&self) -> tect_domain::Result<Option<ModelRouteCatalogue>> {
        Ok(Some(ModelRouteCatalogue {
            schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
            version: 1,
            routes: vec![ModelRoute {
                id: "route-a".into(),
                provider: "configured-provider".into(),
                model: "configured-model".into(),
                effort: "medium".into(),
                enabled: true,
                allowed_matrix_choice_ids: vec!["choice-a".into()],
                allowed_roles: vec!["agent".into()],
                allowed_tools: vec!["code".into()],
                allowed_data_classes: vec!["internal".into()],
                required_host_capabilities: vec!["model-api".into()],
                minimum_budget_units: 10,
                minimum_latency_ms: 50,
            }],
        }))
    }
}

struct TestHost;
impl ModelRouteHostCapabilitiesProvider for TestHost {
    fn host_capabilities(&self) -> tect_domain::Result<ModelRouteFact<Vec<String>>> {
        ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["model-api".into()],
        }
        .fact()
    }
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn current_selected_work_fake_jev_rank_has_sealed_pg_audit_and_disposition() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_pool = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String,i64,String) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_catalog.pg_control_system())"
    ).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(
        identity.0,
        std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap()
    );
    assert_eq!(
        identity.1.to_string(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID").unwrap()
    );
    assert_eq!(
        identity.2,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    crate::admin::migrate(
        &admin_pool,
        &std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap(),
    )
    .await
    .unwrap();
    let created = fixture(&admin_pool, &runtime_pool).await;
    let mut reader = PgUnitOfWork::test_begin(&runtime_pool, created.tenant).await;
    let work = reader
        .approved_work_context(
            created.workspace,
            created.selection.disposition_id,
            created.candidate_set,
            created.caller_request,
            created.work_node,
            created.work_revision,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(work.selection_link.mapped_work_node_id, created.work_node);
    for fact in [&work.role, &work.tool, &work.data_class] {
        assert!(
            matches!(fact, ModelRouteFact::Known { provenance: ModelRouteFactProvenance::Caller { work_node_id, work_node_revision, .. }, .. } if *work_node_id == created.work_node && *work_node_revision == created.work_revision)
        );
    }
    assert!(matches!(
        work.remaining_budget_units,
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Caller { .. },
            value: 20
        }
    ));
    assert!(matches!(
        work.available_latency_ms,
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Caller { .. },
            value: 100
        }
    ));
    assert!(matches!(work.host_capabilities, ModelRouteFact::Unknown));
    drop(reader);

    let store = PgStore::from_pool(runtime_pool.clone());
    let request = PrepareModelRouteRecommendation {
        workspace_id: created.workspace,
        disposition_id: created.selection.disposition_id,
        expected_task_id: created.task,
        expected_task_revision: created.selection.task_revision,
        expected_candidate_set_id: created.candidate_set,
        expected_caller_request_id: created.caller_request,
        expected_mapped_work_node_id: created.work_node,
        expected_mapped_work_node_revision: created.work_revision,
        request_key: format!("route-positive-{}", Uuid::new_v4()),
        requested_route_id: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let mut writer = store.begin(TransactionMode::ReadWrite).await.unwrap();
    writer.authenticate(&created.owner.auth).await.unwrap();
    writer.set_tenant(created.tenant).await.unwrap();
    let mut reader = PgUnitOfWork::test_begin(&runtime_pool, created.tenant).await;
    let prepared = request
        .prepare(
            writer.model_route_recommendation_store().unwrap(),
            &mut reader,
            &TestHost,
            &TestCatalogue,
        )
        .await
        .unwrap();
    assert_eq!(prepared.preparation, ModelRoutePreparation::Prepared);
    assert_eq!(prepared.eligible.as_ref().unwrap().route_ids, ["route-a"]);
    assert_eq!(prepared.routes.observed_actual, None);
    writer.commit().await.unwrap();

    let calls = Arc::new(AtomicUsize::new(0));
    let ranker = FakeJevRanker {
        pool: runtime_pool.clone(),
        tenant: created.tenant,
        calls: calls.clone(),
        malformed: false,
    };
    let invocation = ModelRouteInvocation {
        session_id: created.invocation_session,
    };
    let policy =
        install_synthetic_policy(&store, created.workspace, created.tenant, &created.owner).await;
    let mut denied = store.begin(TransactionMode::ReadWrite).await.unwrap();
    denied.authenticate(&created.owner.auth).await.unwrap();
    denied.set_tenant(created.tenant).await.unwrap();
    assert!(matches!(
        prepare_model_route_send(
            denied.model_route_attempt_store().unwrap(),
            &ranker,
            &prepared,
            invocation
        )
        .await,
        Err(Error::BudgetPolicyInvalid)
    ));
    denied.commit().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let mut send_start = store.begin(TransactionMode::ReadWrite).await.unwrap();
    send_start.authenticate(&created.owner.auth).await.unwrap();
    send_start.set_tenant(created.tenant).await.unwrap();
    let mut trusted = TrustedTestRouteStore::new(
        send_start.model_route_attempt_store().unwrap(),
        &policy,
        created.workspace,
    );
    let (attempted, permit) =
        match prepare_model_route_send(&mut trusted, &ranker, &prepared, invocation)
            .await
            .unwrap()
        {
            ModelRouteSendStart::Started { attempted, permit } => (attempted, permit),
            other => panic!("expected one committed send permit: {other:?}"),
        };
    let observation = attempt_model_route_observed_after_commit(
        send_start.commit(),
        &ranker,
        *attempted.clone(),
        permit.clone(),
    )
    .await
    .unwrap()
    .unwrap();
    let raw = observation.raw.clone();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut repeat = store.begin(TransactionMode::ReadWrite).await.unwrap();
    repeat.authenticate(&created.owner.auth).await.unwrap();
    repeat.set_tenant(created.tenant).await.unwrap();
    assert_eq!(
        prepare_model_route_send(
            repeat.model_route_attempt_store().unwrap(),
            &ranker,
            &prepared,
            invocation
        )
        .await
        .unwrap(),
        ModelRouteSendStart::Replay
    );
    repeat.commit().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let mut raw_writer = store.begin(TransactionMode::ReadWrite).await.unwrap();
    raw_writer.authenticate(&created.owner.auth).await.unwrap();
    raw_writer.set_tenant(created.tenant).await.unwrap();
    let response_sha = seal_model_route_raw_response(
        raw_writer.model_route_attempt_store().unwrap(),
        &permit,
        &raw,
    )
    .await
    .unwrap();
    raw_writer.commit().await.unwrap();
    let mut consumer = store.begin(TransactionMode::ReadWrite).await.unwrap();
    consumer.authenticate(&created.owner.auth).await.unwrap();
    consumer.set_tenant(created.tenant).await.unwrap();
    assert!(
        !consumer
            .model_route_attempt_store()
            .unwrap()
            .consume_budget(&permit, &observation)
            .await
            .unwrap()
    );
    consumer.commit().await.unwrap();
    let mut replay_consumption = store.begin(TransactionMode::ReadWrite).await.unwrap();
    replay_consumption
        .authenticate(&created.owner.auth)
        .await
        .unwrap();
    replay_consumption.set_tenant(created.tenant).await.unwrap();
    assert!(
        !replay_consumption
            .model_route_attempt_store()
            .unwrap()
            .consume_budget(&permit, &observation)
            .await
            .unwrap()
    );
    replay_consumption.commit().await.unwrap();
    let mut parser = store.begin(TransactionMode::ReadWrite).await.unwrap();
    parser.authenticate(&created.owner.auth).await.unwrap();
    parser.set_tenant(created.tenant).await.unwrap();
    let outcome = finalize_model_route_sealed_response(
        parser.model_route_attempt_store().unwrap(),
        &prepared,
        &attempted,
        &permit,
    )
    .await
    .unwrap();
    let ranking = tect_domain::model_route_ranking_from_wire(&attempted.request, &outcome).unwrap();
    parser.commit().await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mut audit = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(created.tenant.to_string())
        .execute(&mut *audit)
        .await
        .unwrap();
    let row = sqlx::query("SELECT state,request_payload,request_sha256,response_payload,response_sha256,parsed_outcome FROM model_route_advisory_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(created.tenant).bind(created.workspace).bind(permit.attempt_id)
        .fetch_one(&mut *audit).await.unwrap();
    assert_eq!(row.try_get::<String, _>("state").unwrap(), "parsed");
    assert_eq!(
        row.try_get::<Vec<u8>, _>("request_payload").unwrap(),
        attempted.request_bytes
    );
    assert_eq!(
        row.try_get::<String, _>("request_sha256").unwrap(),
        attempted.request_sha256
    );
    assert_eq!(row.try_get::<Vec<u8>, _>("response_payload").unwrap(), raw);
    assert_eq!(
        row.try_get::<String, _>("response_sha256").unwrap(),
        response_sha
    );
    assert!(
        row.try_get::<serde_json::Value, _>("parsed_outcome")
            .is_ok()
    );
    let reservation = sqlx::query("SELECT policy_id,policy_version,policy_digest,request_sha256,request_utf8_bytes FROM model_route_budget_reservations WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3")
        .bind(created.tenant).bind(created.workspace).bind(permit.attempt_id)
        .fetch_one(&mut *audit).await.unwrap();
    assert_eq!(
        reservation.try_get::<Uuid, _>("policy_id").unwrap(),
        policy.id()
    );
    assert_eq!(
        reservation.try_get::<i64, _>("policy_version").unwrap(),
        policy.version()
    );
    assert_eq!(
        reservation.try_get::<String, _>("policy_digest").unwrap(),
        policy.digest()
    );
    assert_eq!(
        reservation.try_get::<String, _>("request_sha256").unwrap(),
        attempted.request_sha256
    );
    assert_eq!(
        reservation.try_get::<i64, _>("request_utf8_bytes").unwrap(),
        attempted.request_bytes.len() as i64
    );
    let consumption = sqlx::query("SELECT response_sha256,input_tokens,output_tokens,elapsed_monotonic_ms,unknown_usage,exhausted_after_response FROM model_route_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3")
        .bind(created.tenant).bind(created.workspace).bind(permit.attempt_id)
        .fetch_one(&mut *audit).await.unwrap();
    assert_eq!(
        consumption.try_get::<String, _>("response_sha256").unwrap(),
        response_sha
    );
    assert_eq!(consumption.try_get::<i64, _>("input_tokens").unwrap(), 4);
    assert_eq!(consumption.try_get::<i64, _>("output_tokens").unwrap(), 3);
    assert_eq!(
        consumption
            .try_get::<i64, _>("elapsed_monotonic_ms")
            .unwrap(),
        1
    );
    assert!(!consumption.try_get::<bool, _>("unknown_usage").unwrap());
    assert!(
        !consumption
            .try_get::<bool, _>("exhausted_after_response")
            .unwrap()
    );
    let audit_row = sqlx::query("SELECT count(*) AS rows,sum(call_count) AS calls,bool_and(step='recommendation_before_model_choice') AS correct_step FROM advisory_call_audit WHERE tenant_id=$1 AND workspace_id=$2 AND capability='model_routing'")
        .bind(created.tenant).bind(created.workspace).fetch_one(&mut *audit).await.unwrap();
    assert_eq!(audit_row.try_get::<i64, _>("rows").unwrap(), 1);
    assert_eq!(audit_row.try_get::<i64, _>("calls").unwrap(), 1);
    assert!(audit_row.try_get::<bool, _>("correct_step").unwrap());
    audit.rollback().await.unwrap();
    let mut other_tenant = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(Uuid::new_v4().to_string())
        .execute(&mut *other_tenant)
        .await
        .unwrap();
    let leaked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_call_audit WHERE workspace_id=$1 AND capability='model_routing'",
    )
    .bind(created.workspace)
    .fetch_one(&mut *other_tenant)
    .await
    .unwrap();
    assert_eq!(leaked, 0);
    other_tenant.rollback().await.unwrap();
    let decision = DecideModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: created.workspace,
        preparation_request_key: request.request_key.clone(),
        input: ModelRouteDecisionInput::Ranking(ranking),
    };
    let mut prep_read = PgUnitOfWork::test_begin(&runtime_pool, created.tenant).await;
    let mut decision_write = store.begin(TransactionMode::ReadWrite).await.unwrap();
    decision_write
        .authenticate(&created.owner.auth)
        .await
        .unwrap();
    decision_write.set_tenant(created.tenant).await.unwrap();
    let captured = decision
        .decide(
            &mut prep_read,
            decision_write.model_route_decision_store().unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        captured.outcome,
        ModelRouteDecisionOutcome::Recommended {
            route_id: "route-a".into()
        }
    );
    assert_eq!(
        captured.routes.recommended_route_id.as_deref(),
        Some("route-a")
    );
    assert_eq!(captured.routes.observed_actual, None);
    decision_write.commit().await.unwrap();

    let disposition = DispositionModelRouteRecommendation {
        id: Uuid::new_v4(),
        workspace_id: created.workspace,
        decision_id: decision.id,
        actor_id: created.owner.principal_id,
        action: ModelRouteDispositionAction::Accept,
        rationale: "Synthetic test acceptance; no dispatch".into(),
    };
    let mut final_write = store.begin(TransactionMode::ReadWrite).await.unwrap();
    final_write.authenticate(&created.owner.auth).await.unwrap();
    final_write.set_tenant(created.tenant).await.unwrap();
    let accepted = disposition
        .record(final_write.model_route_decision_store().unwrap())
        .await
        .unwrap();
    assert_eq!(accepted.action, ModelRouteDispositionAction::Accept);
    final_write.commit().await.unwrap();

    let mut replay = store.begin(TransactionMode::ReadWrite).await.unwrap();
    replay.authenticate(&created.owner.auth).await.unwrap();
    replay.set_tenant(created.tenant).await.unwrap();
    assert_eq!(
        disposition
            .record(replay.model_route_decision_store().unwrap())
            .await
            .unwrap(),
        accepted
    );
    let conflict = DispositionModelRouteRecommendation {
        action: ModelRouteDispositionAction::Reject,
        ..disposition
    };
    assert!(matches!(
        conflict
            .record(replay.model_route_decision_store().unwrap())
            .await,
        Err(Error::InputConflict)
    ));
    replay.commit().await.unwrap();
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn model_route_unknown_and_overrun_usage_have_no_visible_advice() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_pool = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String, i64, String) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_catalog.pg_control_system())"
    ).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(
        identity.0,
        std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap()
    );
    assert_eq!(
        identity.1.to_string(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID").unwrap()
    );
    assert_eq!(
        identity.2,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    crate::admin::migrate(
        &admin_pool,
        &std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap(),
    )
    .await
    .unwrap();

    for (label, input_tokens) in [("unknown", None), ("overrun", Some(101))] {
        let created = fixture(&admin_pool, &runtime_pool).await;
        let store = PgStore::from_pool(runtime_pool.clone());
        let request = PrepareModelRouteRecommendation {
            workspace_id: created.workspace,
            disposition_id: created.selection.disposition_id,
            expected_task_id: created.task,
            expected_task_revision: created.selection.task_revision,
            expected_candidate_set_id: created.candidate_set,
            expected_caller_request_id: created.caller_request,
            expected_mapped_work_node_id: created.work_node,
            expected_mapped_work_node_revision: created.work_revision,
            request_key: format!("route-budget-{label}-{}", Uuid::new_v4()),
            requested_route_id: None,
            session_preference: AdvisoryRequestPreference::UseWorkspace,
            request_preference: AdvisoryRequestPreference::UseWorkspace,
        };
        let mut writer = store.begin(TransactionMode::ReadWrite).await.unwrap();
        writer.authenticate(&created.owner.auth).await.unwrap();
        writer.set_tenant(created.tenant).await.unwrap();
        let mut reader = PgUnitOfWork::test_begin(&runtime_pool, created.tenant).await;
        let prepared = request
            .prepare(
                writer.model_route_recommendation_store().unwrap(),
                &mut reader,
                &TestHost,
                &TestCatalogue,
            )
            .await
            .unwrap();
        writer.commit().await.unwrap();
        let policy =
            install_synthetic_policy(&store, created.workspace, created.tenant, &created.owner)
                .await;
        let calls = Arc::new(AtomicUsize::new(0));
        let ranker = FakeJevRanker {
            pool: runtime_pool.clone(),
            tenant: created.tenant,
            calls: calls.clone(),
            malformed: false,
        };
        let invocation = ModelRouteInvocation {
            session_id: created.invocation_session,
        };
        let mut start = store.begin(TransactionMode::ReadWrite).await.unwrap();
        start.authenticate(&created.owner.auth).await.unwrap();
        start.set_tenant(created.tenant).await.unwrap();
        let mut trusted = TrustedTestRouteStore::new(
            start.model_route_attempt_store().unwrap(),
            &policy,
            created.workspace,
        );
        let (attempted, permit) =
            match prepare_model_route_send(&mut trusted, &ranker, &prepared, invocation)
                .await
                .unwrap()
            {
                ModelRouteSendStart::Started { attempted, permit } => (attempted, permit),
                other => panic!("expected send: {other:?}"),
            };
        let mut observation = attempt_model_route_observed_after_commit(
            start.commit(),
            &ranker,
            *attempted.clone(),
            permit.clone(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        observation.input_tokens = input_tokens;
        let mut seal = store.begin(TransactionMode::ReadWrite).await.unwrap();
        seal.authenticate(&created.owner.auth).await.unwrap();
        seal.set_tenant(created.tenant).await.unwrap();
        seal_model_route_raw_response(
            seal.model_route_attempt_store().unwrap(),
            &permit,
            &observation.raw,
        )
        .await
        .unwrap();
        seal.commit().await.unwrap();
        let mut consume = store.begin(TransactionMode::ReadWrite).await.unwrap();
        consume.authenticate(&created.owner.auth).await.unwrap();
        consume.set_tenant(created.tenant).await.unwrap();
        assert!(
            consume
                .model_route_attempt_store()
                .unwrap()
                .consume_budget(&permit, &observation)
                .await
                .unwrap()
        );
        consume.commit().await.unwrap();
        let mut replay = store.begin(TransactionMode::ReadWrite).await.unwrap();
        replay.authenticate(&created.owner.auth).await.unwrap();
        replay.set_tenant(created.tenant).await.unwrap();
        assert!(
            replay
                .model_route_attempt_store()
                .unwrap()
                .consume_budget(&permit, &observation)
                .await
                .unwrap()
        );
        assert_eq!(
            prepare_model_route_send(
                replay.model_route_attempt_store().unwrap(),
                &ranker,
                &prepared,
                invocation
            )
            .await
            .unwrap(),
            ModelRouteSendStart::Replay
        );
        assert_eq!(
            replay
                .model_route_attempt_store()
                .unwrap()
                .by_preparation(created.workspace, &request.request_key, invocation)
                .await
                .unwrap()
                .unwrap()
                .state,
            tect_application::ModelRouteAttemptState::BudgetExhausted
        );
        assert_eq!(
            finalize_model_route_sealed_response(
                replay.model_route_attempt_store().unwrap(),
                &prepared,
                &attempted,
                &permit
            )
            .await,
            Err(Error::BudgetPolicyInvalid)
        );
        replay.commit().await.unwrap();
        let mut audit = runtime_pool.begin().await.unwrap();
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(created.tenant.to_string())
            .execute(&mut *audit)
            .await
            .unwrap();
        let decision_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM model_route_decisions WHERE tenant_id=$1 AND workspace_id=$2 AND preparation_request_key=$3")
            .bind(created.tenant).bind(created.workspace).bind(&request.request_key)
            .fetch_one(&mut *audit).await.unwrap();
        assert_eq!(decision_count, 0);
        let disposition_count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM model_route_dispositions WHERE tenant_id=$1 AND workspace_id=$2",
        )
        .bind(created.tenant)
        .bind(created.workspace)
        .fetch_one(&mut *audit)
        .await
        .unwrap();
        assert_eq!(disposition_count, 0);
        audit.rollback().await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
