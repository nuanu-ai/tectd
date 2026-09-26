use super::*;

#[tokio::test]
async fn one_call_after_commit_raw_sealed_before_rank_and_replay_no_send() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let committed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider {
        calls: calls.clone(),
        committed: committed.clone(),
        fail: false,
    };
    let mut store = Memory::default();
    let ModelRouteSendStart::Started { attempted, permit } =
        prepare_model_route_send(&mut store, &provider, &saved, invocation())
            .await
            .unwrap()
    else {
        panic!("start")
    };
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let observation = attempt_model_route_observed_after_commit(
        async {
            committed.store(true, Ordering::SeqCst);
            Ok(())
        },
        &provider,
        *attempted.clone(),
        permit.clone(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    seal_model_route_raw_response(&mut store, &permit, &observation.raw)
        .await
        .unwrap();
    assert!(!store.consume_budget(&permit, &observation).await.unwrap());
    let outcome = finalize_model_route_sealed_response(&mut store, &saved, &attempted, &permit)
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        ModelRouteRankingWireOutcome::Ranked { .. }
    ));
    let evidence = store.evidence.as_ref().unwrap();
    assert_eq!(
        evidence.verify(&saved).unwrap().unwrap().ranked_route_ids,
        ["route-a"]
    );
    assert_eq!(saved.routes.recommended_route_id, None);
    assert_eq!(saved.routes.observed_actual, None);
    assert_eq!(
        prepare_model_route_send(&mut store, &provider, &saved, invocation())
            .await
            .unwrap(),
        ModelRouteSendStart::Replay
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn disabled_and_unknown_are_no_call_without_provider_attempt() {
    let committed = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider {
        calls: calls.clone(),
        committed,
        fail: false,
    };
    let mut store = Memory::default();
    assert_eq!(
        prepare_model_route_send(
            &mut store,
            &DisabledModelRouteRankingProvider,
            &prepared(ModelRoutePreparation::Prepared),
            invocation(),
        )
        .await
        .unwrap(),
        ModelRouteSendStart::NoCall(ModelRouteRunNoCall::ProviderUnavailable)
    );
    assert_eq!(
        prepare_model_route_send(
            &mut store,
            &provider,
            &prepared(ModelRoutePreparation::UnknownWorkFacts),
            invocation(),
        )
        .await
        .unwrap(),
        ModelRouteSendStart::NoCall(ModelRouteRunNoCall::Preparation(
            ModelRoutePreparation::UnknownWorkFacts
        ))
    );
    assert_eq!(store.no_calls, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn no_trusted_policy_blocks_send_and_unknown_or_overrun_usage_blocks_ranking() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let provider = FakeProvider {
        calls: Arc::new(AtomicUsize::new(0)),
        committed: Arc::new(AtomicBool::new(true)),
        fail: false,
    };
    let mut no_policy = Memory {
        policy_enabled: false,
        ..Memory::default()
    };
    assert_eq!(
        prepare_model_route_send(&mut no_policy, &provider, &saved, invocation()).await,
        Err(Error::BudgetPolicyInvalid)
    );
    assert!(no_policy.sent.is_none());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

    for (input_tokens, output_tokens) in [(None, Some(3)), (Some(101), Some(3))] {
        let mut store = Memory::default();
        let ModelRouteSendStart::Started { attempted, permit } =
            prepare_model_route_send(&mut store, &provider, &saved, invocation())
                .await
                .unwrap()
        else {
            panic!("start")
        };
        let raw = provider
            .attempt_prepared(*attempted.clone(), permit.clone())
            .await
            .unwrap();
        seal_model_route_raw_response(&mut store, &permit, &raw)
            .await
            .unwrap();
        let observation = ModelRouteProviderObservation {
            response_complete: None,
            original_transport_context: None,
            raw: raw.clone(),
            http_status: None,
            input_tokens,
            output_tokens,
            elapsed_monotonic_ms: Some(1),
        };
        assert!(store.consume_budget(&permit, &observation).await.unwrap());
        assert!(store.consume_budget(&permit, &observation).await.unwrap());
        assert_eq!(
            store.consumption_healthy(&permit).await.unwrap(),
            Some(false)
        );
        assert_eq!(
            finalize_model_route_sealed_response(&mut store, &saved, &attempted, &permit).await,
            Err(Error::BudgetPolicyInvalid)
        );
        assert!(store.evidence.is_none());
    }
}

#[tokio::test]
async fn malformed_sealed_raw_and_uncertain_send_never_retry() {
    let saved = prepared(ModelRoutePreparation::Prepared);
    let committed = Arc::new(AtomicBool::new(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = FakeProvider {
        calls: calls.clone(),
        committed,
        fail: true,
    };
    let mut store = Memory::default();
    let ModelRouteSendStart::Started { attempted, permit } =
        prepare_model_route_send(&mut store, &provider, &saved, invocation())
            .await
            .unwrap()
    else {
        panic!("start")
    };
    assert_eq!(
        attempt_model_route_after_commit(
            async { Ok(()) },
            &provider,
            *attempted.clone(),
            permit.clone()
        )
        .await
        .unwrap(),
        Err(Error::TransportUnavailable)
    );
    store.mark_send_unknown(&permit).await.unwrap();
    assert_eq!(
        prepare_model_route_send(&mut store, &provider, &saved, invocation())
            .await
            .unwrap(),
        ModelRouteSendStart::Replay
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let mut second = Memory::default();
    let ModelRouteSendStart::Started { attempted, permit } =
        prepare_model_route_send(&mut second, &provider, &saved, invocation())
            .await
            .unwrap()
    else {
        panic!("start")
    };
    seal_model_route_raw_response(&mut second, &permit, b"{malformed")
        .await
        .unwrap();
    assert!(
        !second
            .consume_budget(
                &permit,
                &ModelRouteProviderObservation {
                    response_complete: None,
                    original_transport_context: None,
                    raw: b"{malformed".to_vec(),
                    http_status: None,
                    input_tokens: Some(1),
                    output_tokens: Some(1),
                    elapsed_monotonic_ms: Some(1)
                }
            )
            .await
            .unwrap()
    );
    assert_eq!(
        finalize_model_route_sealed_response(&mut second, &saved, &attempted, &permit).await,
        Err(Error::InvalidArguments)
    );
    assert_eq!(second.sealed.as_deref(), Some(b"{malformed".as_slice()));
    assert_eq!(
        prepare_model_route_send(&mut second, &provider, &saved, invocation())
            .await
            .unwrap(),
        ModelRouteSendStart::Replay
    );
}

#[test]
fn model_route_wire_and_digest_golden_vectors() {
    let eligible = prepared(ModelRoutePreparation::Prepared);
    let no_call = ModelRouteAttemptSnapshot {
        attempt_id: Uuid::from_u128(107),
        state: crate::ModelRouteAttemptState::NoCall,
        no_call_reason: Some("provider_unavailable".into()),
        request_sha256: None,
        response_sha256: None,
    };
    let raw_sealed = ModelRouteAttemptSnapshot {
        attempt_id: Uuid::from_u128(108),
        state: crate::ModelRouteAttemptState::RawSealed,
        no_call_reason: None,
        request_sha256: Some("a".repeat(64)),
        response_sha256: Some("b".repeat(64)),
    };
    let abstain = crate::CapturedModelRouteDecision {
        id: Uuid::from_u128(109),
        prepared: eligible.clone(),
        input: crate::ModelRouteDecisionInput::Abstain,
        outcome: crate::ModelRouteDecisionOutcome::Abstained {
            reason: crate::ModelRouteAbstainReason::Explicit,
        },
        routes: eligible.routes.clone(),
    };
    for (name, bytes, golden, digest) in [
        (
            "eligible",
            serde_json::to_vec(&eligible).unwrap(),
            include_str!("eligible_golden.json"),
            "b215dfcc7c0e36f013395367627a2d84299091cae971db1aac427ab72b491a47",
        ),
        (
            "no_call",
            serde_json::to_vec(&no_call).unwrap(),
            include_str!("no_call_golden.json"),
            "19676169e39e30227cde58b96b894b9dec95e85f1cc0cdb918122897a3ff8d35",
        ),
        (
            "raw_sealed",
            serde_json::to_vec(&raw_sealed).unwrap(),
            include_str!("raw_sealed_golden.json"),
            "84cd0d1ace27c19da5a0f923dabd75dfbd3454b24c0c2a32441c3da897655a7a",
        ),
        (
            "abstain",
            serde_json::to_vec(&abstain).unwrap(),
            include_str!("abstain_golden.json"),
            "6afcc2c6cb52c099939edaed2ac696e5aca33175dd2347a47bb80e70fe2e9973",
        ),
    ] {
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            golden.trim_end(),
            "{name}"
        );
        assert_eq!(model_route_wire_sha256(&bytes), digest, "{name}");
    }
    assert_eq!(
        serde_json::from_str::<PreparedModelRouteRecommendation>(include_str!(
            "eligible_golden.json"
        ))
        .unwrap(),
        eligible
    );
    assert_eq!(
        serde_json::from_str::<ModelRouteAttemptSnapshot>(include_str!("no_call_golden.json"))
            .unwrap(),
        no_call
    );
    assert_eq!(
        serde_json::from_str::<ModelRouteAttemptSnapshot>(include_str!("raw_sealed_golden.json"))
            .unwrap(),
        raw_sealed
    );
    assert_eq!(
        serde_json::from_str::<crate::CapturedModelRouteDecision>(include_str!(
            "abstain_golden.json"
        ))
        .unwrap(),
        abstain
    );
}
