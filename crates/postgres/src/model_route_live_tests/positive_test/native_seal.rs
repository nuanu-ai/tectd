use super::*;
use tect_application::{ModelRouteSealedRankingEvidence, finalize_model_route_provider_response};
use tect_domain::{ModelRouteRankingWireOutcome, model_route_wire_sha256};

struct NativeCodec;

pub(super) async fn choose_profile(
    store: &PgStore,
    created: &super::super::positive::Fixture,
    profile: &str,
) {
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&created.owner.auth).await.unwrap();
    tx.set_tenant(created.tenant).await.unwrap();
    tx.configure_advisory(
        created.workspace,
        created.owner.principal_id,
        created.invocation_session,
        &tect_domain::ConfigureWorkspaceAdvisory {
            expected_revision: 0,
            mode: tect_domain::WorkspaceAdvisoryMode::Optional,
            provider_profile_ref: Some(tect_domain::AdvisoryProviderProfileRef {
                id: profile.into(),
            }),
            model_configuration: Some(tect_domain::AdvisoryModelConfiguration {
                model: "native-model".into(),
            }),
        },
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}
#[async_trait]
impl ModelRouteRankingProvider for NativeCodec {
    fn required_profile(&self) -> Option<&str> {
        Some("native-test")
    }
    fn prepare(
        &self,
        saved: &tect_application::PreparedModelRouteRecommendation,
    ) -> tect_domain::Result<ModelRoutePreparedAttempt> {
        ModelRoutePreparedAttempt::native(
            ModelRouteRankingWireRequest::new(
                saved.workspace_id,
                &saved.request_key,
                &saved.work,
                saved.catalogue.as_ref().unwrap(),
                saved.eligible.as_ref().unwrap(),
                "native-model",
            )?,
            b"immutable native outbound wire".to_vec(),
            "native-test-v1".into(),
        )
    }
    async fn attempt_prepared(
        &self,
        _: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> tect_domain::Result<Vec<u8>> {
        panic!("recovery/capture must not send")
    }
    fn parse_sealed(
        &self,
        attempted: &ModelRoutePreparedAttempt,
        observation: &ModelRouteProviderObservation,
    ) -> tect_domain::Result<ModelRouteRankingWireOutcome> {
        if attempted.adapter_identity.as_deref() != Some("native-test-v1")
            || attempted.request_bytes != b"immutable native outbound wire"
            || observation.raw != b"native provider exact response"
        {
            return Err(Error::InvalidArguments);
        }
        Ok(ModelRouteRankingWireOutcome::Ranked {
            route_ids: attempted.request.binding.eligible_route_ids.clone(),
        })
    }
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PG96 and TECT_TEST_* URLs"]
async fn native_exact_seal_restores_and_rederives_before_immutable_capture() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let identity: (String,i64,String) = sqlx::query_as("SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_catalog.pg_control_system())").fetch_one(&admin).await.unwrap();
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
    let migration: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(migration, 96);
    let created = fixture(&admin, &runtime).await;
    let store = PgStore::from_pool(runtime.clone());
    choose_profile(&store, &created, "native-test").await;
    let prepared = negative_cases::prepare_case(&store, &runtime, &created, "native-seal").await;
    let policy =
        install_synthetic_policy(&store, created.workspace, created.tenant, &created.owner).await;
    let attempted = NativeCodec.prepare(&prepared).unwrap();
    let invocation = ModelRouteInvocation {
        session_id: created.invocation_session,
    };
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&created.owner.auth).await.unwrap();
    tx.set_tenant(created.tenant).await.unwrap();
    assert_eq!(
        tx.model_route_attempt_store()
            .unwrap()
            .begin_send(&prepared, invocation, &attempted, &policy, None)
            .await,
        Err(Error::TransportUnavailable)
    );
    let permit = tx
        .model_route_attempt_store()
        .unwrap()
        .begin_send(
            &prepared,
            invocation,
            &attempted,
            &policy,
            Some("native-test"),
        )
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    let observation = ModelRouteProviderObservation {
        response_complete: Some(true),
        original_transport_context: Some(tect_application::AdvisoryProviderTransportContext {
            send_certainty: tect_domain::AdvisorySendCertainty::Sent,
            outcome: tect_domain::AdvisoryDispatchOutcome::ProviderResponse,
            raw_response_ref: Some(format!(
                "sha256:{}",
                model_route_wire_sha256(b"native provider exact response")
            )),
            provider_failure_code: None,
        }),
        raw: b"native provider exact response".to_vec(),
        http_status: Some(200),
        input_tokens: Some(4),
        output_tokens: Some(3),
        elapsed_monotonic_ms: Some(2),
    };
    store
        .seal_committed_model_route_observation(created.tenant, &permit, &observation)
        .await
        .unwrap();
    store
        .seal_committed_model_route_observation(created.tenant, &permit, &observation)
        .await
        .unwrap();
    let mut bad = observation.clone();
    bad.http_status = Some(503);
    assert_eq!(
        store
            .seal_committed_model_route_observation(created.tenant, &permit, &bad)
            .await,
        Err(Error::InputConflict)
    );
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&created.owner.auth).await.unwrap();
    tx.set_tenant(created.tenant).await.unwrap();
    let attempts = tx.model_route_attempt_store().unwrap();
    let recovered = attempts
        .recover_raw_sealed(created.workspace, &prepared.request_key, invocation)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered, (attempted.clone(), permit.clone()));
    assert_eq!(
        attempts.sealed_observation(&permit).await.unwrap(),
        Some(observation.clone())
    );
    assert!(
        !attempts
            .consume_budget(&permit, &observation)
            .await
            .unwrap()
    );
    assert!(
        !attempts
            .consume_budget(&permit, &observation)
            .await
            .unwrap()
    );
    let mut forged = ModelRouteSealedRankingEvidence {
        permit: permit.clone(),
        attempted: attempted.clone(),
        raw_response: observation.raw.clone(),
        response_sha256: model_route_wire_sha256(&observation.raw),
        outcome: ModelRouteRankingWireOutcome::Ranked {
            route_ids: attempted.request.binding.eligible_route_ids.clone(),
        },
    };
    assert_eq!(
        attempts.capture_sealed_outcome(&forged).await,
        Err(Error::InputConflict)
    );
    forged.raw_response = b"forged native body".to_vec();
    forged.response_sha256 = model_route_wire_sha256(&forged.raw_response);
    assert!(
        attempts
            .capture_provider_outcome(&forged, &NativeCodec)
            .await
            .is_err()
    );
    finalize_model_route_provider_response(attempts, &NativeCodec, &prepared, &attempted, &permit)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&created.owner.auth).await.unwrap();
    tx.set_tenant(created.tenant).await.unwrap();
    let proof = tx
        .model_route_decision_store()
        .unwrap()
        .sealed_provider_ranking(created.workspace, &prepared.request_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(proof.attempted, attempted);
    assert_eq!(proof.raw_response, observation.raw);
    assert!(proof.validate_material(&prepared).unwrap().is_some());
    assert!(prepared.routes.observed_actual.is_none());
    tx.commit().await.unwrap();
    let counts: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM model_route_budget_reservations WHERE attempt_id=$1),(SELECT count(*) FROM model_route_budget_consumptions WHERE attempt_id=$1)").bind(permit.attempt_id).fetch_one(&admin).await.unwrap();
    assert_eq!(counts, (1, 1));
    // A correct digest and valid permutation cannot authenticate a wrong native body.
    let prepared_bad =
        negative_cases::prepare_case(&store, &runtime, &created, "native-wrong-body").await;
    let attempted_bad = NativeCodec.prepare(&prepared_bad).unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&created.owner.auth).await.unwrap();
    tx.set_tenant(created.tenant).await.unwrap();
    let permit_bad = tx
        .model_route_attempt_store()
        .unwrap()
        .begin_send(
            &prepared_bad,
            invocation,
            &attempted_bad,
            &policy,
            Some("native-test"),
        )
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    let bad = ModelRouteProviderObservation {
        raw: b"sealed incorrect provider body".to_vec(),
        ..observation.clone()
    };
    store
        .seal_committed_model_route_observation(created.tenant, &permit_bad, &bad)
        .await
        .unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&created.owner.auth).await.unwrap();
    tx.set_tenant(created.tenant).await.unwrap();
    let attempts = tx.model_route_attempt_store().unwrap();
    assert!(!attempts.consume_budget(&permit_bad, &bad).await.unwrap());
    let forged = ModelRouteSealedRankingEvidence {
        permit: permit_bad.clone(),
        attempted: attempted_bad.clone(),
        raw_response: bad.raw.clone(),
        response_sha256: model_route_wire_sha256(&bad.raw),
        outcome: ModelRouteRankingWireOutcome::Ranked {
            route_ids: attempted_bad.request.binding.eligible_route_ids.clone(),
        },
    };
    assert_eq!(
        attempts
            .capture_provider_outcome(&forged, &NativeCodec)
            .await,
        Err(Error::InvalidArguments)
    );
    tx.commit().await.unwrap();
    let state: String =
        sqlx::query_scalar("SELECT state FROM model_route_advisory_attempts WHERE id=$1")
            .bind(permit_bad.attempt_id)
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(state, "raw_sealed");
    // Historical raw-only seals retain unknown elapsed; callers cannot backfill it.
    let unknown = fixture(&admin, &runtime).await;
    choose_profile(&store, &unknown, "native-test").await;
    let prepared_unknown =
        negative_cases::prepare_case(&store, &runtime, &unknown, "unknown-elapsed").await;
    let policy_unknown =
        install_synthetic_policy(&store, unknown.workspace, unknown.tenant, &unknown.owner).await;
    let attempted_unknown = NativeCodec.prepare(&prepared_unknown).unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&unknown.owner.auth).await.unwrap();
    tx.set_tenant(unknown.tenant).await.unwrap();
    let permit_unknown = tx
        .model_route_attempt_store()
        .unwrap()
        .begin_send(
            &prepared_unknown,
            ModelRouteInvocation {
                session_id: unknown.invocation_session,
            },
            &attempted_unknown,
            &policy_unknown,
            Some("native-test"),
        )
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();
    store
        .seal_committed_model_route_response(unknown.tenant, &permit_unknown, &observation.raw)
        .await
        .unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&unknown.owner.auth).await.unwrap();
    tx.set_tenant(unknown.tenant).await.unwrap();
    let attempts = tx.model_route_attempt_store().unwrap();
    let mut restored = attempts
        .sealed_observation(&permit_unknown)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(restored.elapsed_monotonic_ms, None);
    assert_eq!(restored.http_status, None);
    assert_eq!(restored.response_complete, None);
    assert_eq!(restored.original_transport_context, None);
    restored.input_tokens = Some(4);
    restored.output_tokens = Some(3);
    let mut backfill = restored.clone();
    backfill.elapsed_monotonic_ms = Some(1);
    assert_eq!(
        attempts.consume_budget(&permit_unknown, &backfill).await,
        Err(Error::InputConflict)
    );
    backfill = restored.clone();
    backfill.http_status = Some(200);
    assert_eq!(
        attempts.consume_budget(&permit_unknown, &backfill).await,
        Err(Error::InputConflict)
    );
    assert!(
        attempts
            .consume_budget(&permit_unknown, &restored)
            .await
            .unwrap()
    );
    assert!(
        attempts
            .consume_budget(&permit_unknown, &restored)
            .await
            .unwrap()
    );
    assert_eq!(
        attempts.consumption_healthy(&permit_unknown).await.unwrap(),
        Some(false)
    );
    tx.commit().await.unwrap();
}
