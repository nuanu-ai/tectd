use super::*;

#[test]
fn preflight_mismatch_and_oversize_are_auditable_no_call_reasons() {
    let request = fixture_scope_request();
    let context = crate::ScopeAdviceProviderContext::for_test(request);
    let mut config = no_call_config(WorkspaceAdvisoryMode::Optional);
    config.provider_profile_ref = Some(AdvisoryProviderProfileRef {
        id: "selected-profile".into(),
    });
    config.model_configuration = Some(AdvisoryModelConfiguration {
        model: "selected-model".into(),
    });

    let mismatched = PreflightFixtureProvider {
        profile: "other-profile".into(),
        model: "selected-model".into(),
        maximum_body_bytes: usize::MAX,
        prepare_calls: Arc::new(AtomicUsize::new(0)),
        attempt_calls: Arc::new(AtomicUsize::new(0)),
    };
    assert_eq!(
        prepare_scope_advice_attempt(&mismatched, &context, &config),
        Err(AdvisoryReason::ProviderUnconfigured)
    );
    assert_eq!(mismatched.prepare_calls.load(Ordering::SeqCst), 1);
    assert_eq!(mismatched.attempt_calls.load(Ordering::SeqCst), 0);

    let mismatched_model = PreflightFixtureProvider {
        profile: "selected-profile".into(),
        model: "other-model".into(),
        maximum_body_bytes: usize::MAX,
        prepare_calls: Arc::new(AtomicUsize::new(0)),
        attempt_calls: Arc::new(AtomicUsize::new(0)),
    };
    assert_eq!(
        prepare_scope_advice_attempt(&mismatched_model, &context, &config),
        Err(AdvisoryReason::ProviderUnconfigured)
    );
    assert_eq!(mismatched_model.attempt_calls.load(Ordering::SeqCst), 0);

    let oversized = PreflightFixtureProvider {
        profile: "selected-profile".into(),
        model: "selected-model".into(),
        maximum_body_bytes: 1,
        prepare_calls: Arc::new(AtomicUsize::new(0)),
        attempt_calls: Arc::new(AtomicUsize::new(0)),
    };
    assert_eq!(
        prepare_scope_advice_attempt(&oversized, &context, &config),
        Err(AdvisoryReason::DeterministicInputInvalid)
    );
    assert_eq!(oversized.attempt_calls.load(Ordering::SeqCst), 0);
    assert!(tect_domain::advisory_reason_matches_state(
        AdvisoryOpportunityState::NoCall,
        AdvisoryReason::ProviderUnconfigured
    ));
    assert!(tect_domain::advisory_reason_matches_state(
        AdvisoryOpportunityState::NoCall,
        AdvisoryReason::DeterministicInputInvalid
    ));

    let source = ORCHESTRATION_SOURCE;
    let preflight = source
        .find("let prepared_attempt = match prepare_scope_advice_attempt")
        .unwrap();
    let no_call_capture = source[preflight..]
        .find("capture_advisory_opportunity(")
        .unwrap()
        + preflight;
    let authorize = source
        .find(".authorize_advisory_dispatch(&lifecycle")
        .unwrap();
    assert!(preflight < no_call_capture && no_call_capture < authorize);
    assert!(source[no_call_capture..authorize].contains("AdvisoryOpportunityState::NoCall"));
}

#[test]
fn request_key_replay_gate_precedes_wire_preparation() {
    let source = ORCHESTRATION_SOURCE;
    let locked_gate = source.find("Recheck under the session lock").unwrap();
    let request_lookup = source[locked_gate..]
        .find("scope_advisory_manifest_by_request_key")
        .unwrap()
        + locked_gate;
    let replay = source[request_lookup..]
        .find("replay_authored_scope_advisory(")
        .unwrap()
        + request_lookup;
    let prepare = source
        .find("let prepared_attempt = match prepare_scope_advice_attempt")
        .unwrap();
    let send = source.find(".observe_prepared(").unwrap();
    assert!(locked_gate < request_lookup && request_lookup < replay);
    assert!(replay < prepare && prepare < send);
}

#[test]
fn attempted_transport_error_is_sent_unknown_and_never_downgraded() {
    let observation = provider_error_observation(ScopeAdviceProviderError::SentUnknown {
        raw_response_ref: Some("fixture:uncertain:1".into()),
        latency_ms: 17,
    });
    assert_eq!(
        observation.send_certainty,
        AdvisorySendCertainty::SentUnknown
    );
    assert_eq!(
        observation.outcome,
        AdvisoryDispatchOutcome::ProviderFailure
    );
    assert_eq!(
        observation.raw_response_ref.as_deref(),
        Some("fixture:uncertain:1")
    );
    assert_eq!(observation.latency_ms, Some(17));
    assert_eq!(
        provider_error_observation(ScopeAdviceProviderError::ProvenNotSent).send_certainty,
        AdvisorySendCertainty::NotSent
    );
}

#[test]
fn invalid_success_certainty_fails_closed_as_sent_unknown() {
    let observation = normalize_provider_success(ScopeAdviceProviderObservation {
        send_certainty: AdvisorySendCertainty::NotSent,
        outcome: AdvisoryDispatchOutcome::ProviderFailure,
        answers: None,
        response_payload: None,
        input_tokens: None,
        output_tokens: None,
        latency_ms: Some(9),
        raw_response_ref: Some("fixture:invalid-success:1".into()),
        failure_reason: None,
    });
    assert_eq!(
        observation.send_certainty,
        AdvisorySendCertainty::SentUnknown
    );
    assert_eq!(
        observation.raw_response_ref.as_deref(),
        Some("fixture:invalid-success:1")
    );
    assert_eq!(observation.latency_ms, Some(9));
}

#[test]
fn typed_sent_failure_keeps_partial_transport_evidence() {
    let observation = ScopeAdviceProviderObservation {
        send_certainty: AdvisorySendCertainty::Sent,
        outcome: AdvisoryDispatchOutcome::ProviderFailure,
        answers: None,
        response_payload: Some(b"partial".to_vec()),
        input_tokens: None,
        output_tokens: None,
        latency_ms: Some(23),
        raw_response_ref: Some("fixture:body-read:observed-7:retained-7".into()),
        failure_reason: Some(crate::ScopeAdviceProviderFailureReason::ResponseBodyRead),
    };
    assert_eq!(normalize_provider_success(observation.clone()), observation);
}

#[test]
fn score_contract_is_discrete_and_has_no_product_effect_authority() {
    assert_eq!(ScopeAdviceScoreBand::Conflict.ordinal(), 0);
    assert_eq!(ScopeAdviceScoreBand::StrongFit.ordinal(), 3);
    let _normalized_only = (ScopeAdviceChoice::Preferred, ConfidenceBasisPoints(10_000));
    let source = ORCHESTRATION_SOURCE;
    assert!(!source.contains("scope_caller.call"));
    assert!(!source.contains("scope_verifier.verify"));
    assert_eq!(source.matches(".observe_prepared(").count(), 1);
    let clock = source
        .find("let monotonic_start = std::time::Instant::now()")
        .unwrap();
    let observe = source.find(".observe_prepared(").unwrap();
    let elapsed = source
        .find("let elapsed = i64::try_from(monotonic_start.elapsed().as_millis())")
        .unwrap();
    let seal = source
        .find("seal_committed_advisory_observation(&continuation, &raw.receipt, elapsed)")
        .unwrap();
    assert!(clock < observe && observe < elapsed && elapsed < seal);
}

#[test]
fn no_host_route_or_http_adapter_is_added() {
    let runtime = include_str!("../../scope_advisory_runtime.rs");
    assert!(!runtime.contains("reqwest"));
    assert!(!runtime.contains("hyper::"));
    assert!(!runtime.contains(&["Jev", "Dto"].concat()));
}
