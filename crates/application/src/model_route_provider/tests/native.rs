use super::*;

struct Native;
struct PreparationFailure;
#[async_trait]
impl ModelRouteRankingProvider for PreparationFailure {
    fn prepare(&self, _: &PreparedModelRouteRecommendation) -> Result<ModelRoutePreparedAttempt> {
        Err(Error::InvalidArguments)
    }
    async fn attempt_prepared(
        &self,
        _: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        panic!("preflight failure must not send")
    }
}

#[tokio::test]
async fn provider_unavailable_or_preparation_failure_is_durable_no_call_before_budget() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    for provider in [
        &DisabledModelRouteRankingProvider as &dyn ModelRouteRankingProvider,
        &PreparationFailure,
    ] {
        let mut store = Memory::default();
        let started = prepare_model_route_send(&mut store, provider, &saved, invocation())
            .await
            .unwrap();
        assert_eq!(
            started,
            ModelRouteSendStart::NoCall(ModelRouteRunNoCall::ProviderUnavailable)
        );
        assert_eq!(store.no_calls, 1);
        assert!(store.sent.is_none());
        assert!(store.consumed.is_none());
    }
}
#[async_trait]
impl ModelRouteRankingProvider for Native {
    fn prepare(
        &self,
        saved: &PreparedModelRouteRecommendation,
    ) -> Result<ModelRoutePreparedAttempt> {
        let generic = FakeProvider {
            committed: Arc::new(AtomicBool::new(false)),
            calls: Arc::new(AtomicUsize::new(0)),
            fail: false,
        }
        .prepare(saved)?;
        ModelRoutePreparedAttempt::native(
            generic.request,
            b"native exact wire".to_vec(),
            "native-test-v1".into(),
        )
    }
    async fn attempt_prepared(
        &self,
        _: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        panic!("sealed continuation must not send")
    }
    fn parse_sealed(
        &self,
        attempted: &ModelRoutePreparedAttempt,
        observation: &ModelRouteProviderObservation,
    ) -> Result<ModelRouteRankingWireOutcome> {
        if attempted.adapter_identity.as_deref() != Some("native-test-v1")
            || attempted.request_bytes != b"native exact wire"
            || observation.raw != b"native result"
        {
            return Err(Error::InvalidArguments);
        }
        Ok(ModelRouteRankingWireOutcome::Ranked {
            route_ids: attempted.request.binding.eligible_route_ids.clone(),
        })
    }
}

#[tokio::test]
async fn native_exact_wire_seal_and_typed_outcome_without_raw_rewrite() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let attempted = Native.prepare(&saved).unwrap();
    attempted.verify(&saved).unwrap();
    assert_ne!(attempted.request_bytes, attempted.request.bytes().unwrap());
    let mut store = Memory::default();
    let permit = store
        .begin_send(&saved, invocation(), &attempted, &test_policy())
        .await
        .unwrap()
        .unwrap();
    let observation = ModelRouteProviderObservation {
        raw: b"native result".to_vec(),
        http_status: Some(200),
        input_tokens: Some(3),
        output_tokens: Some(2),
        elapsed_monotonic_ms: Some(1),
    };
    store.seal_observation(&permit, &observation).await.unwrap();
    assert!(!store.consume_budget(&permit, &observation).await.unwrap());
    let outcome =
        finalize_model_route_provider_response(&mut store, &Native, &saved, &attempted, &permit)
            .await
            .unwrap();
    assert!(matches!(
        outcome,
        ModelRouteRankingWireOutcome::Ranked { .. }
    ));
    let evidence = store.evidence.unwrap();
    assert_eq!(evidence.raw_response, b"native result");
    assert!(evidence.verify(&saved).is_err());
    assert!(evidence.validate_material(&saved).unwrap().is_some());
    assert!(saved.routes.observed_actual.is_none());
}

#[tokio::test]
async fn non2xx_valid_native_result_is_accounted_but_never_parsed() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let attempted = Native.prepare(&saved).unwrap();
    let mut store = Memory::default();
    let permit = store
        .begin_send(&saved, invocation(), &attempted, &test_policy())
        .await
        .unwrap()
        .unwrap();
    let observation = ModelRouteProviderObservation {
        raw: b"native result".to_vec(),
        http_status: Some(503),
        input_tokens: Some(3),
        output_tokens: Some(2),
        elapsed_monotonic_ms: Some(1),
    };
    store.seal_observation(&permit, &observation).await.unwrap();
    assert!(!store.consume_budget(&permit, &observation).await.unwrap());
    assert!(matches!(
        finalize_model_route_provider_response(&mut store, &Native, &saved, &attempted, &permit)
            .await,
        Err(Error::InvalidArguments)
    ));
    assert!(store.evidence.is_none());
    assert!(!store.consume_budget(&permit, &observation).await.unwrap());
}
