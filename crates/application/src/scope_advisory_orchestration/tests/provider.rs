use super::*;

#[tokio::test]
async fn fixture_provider_returns_normalized_answers_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let received_body = Arc::new(Mutex::new(None));
    let observation = ScopeAdviceProviderObservation {
        send_certainty: AdvisorySendCertainty::Sent,
        outcome: AdvisoryDispatchOutcome::ProviderResponse,
        answers: Some(NormalizedScopeAdviceAnswers {
            comparative_disposition: None,
            answers: Vec::new(),
        }),
        response_payload: Some(b"duplicate-aware adapter output".to_vec()),
        input_tokens: Some(1),
        output_tokens: Some(1),
        latency_ms: Some(4),
        raw_response_ref: Some("fixture:raw:1".into()),
        failure_reason: None,
    };
    let provider = FixtureProvider {
        calls: calls.clone(),
        received_body: received_body.clone(),
        observation: observation.clone(),
    };
    let verified_policy = super::budget::syntactic_policy(65_536);
    let request = ScopeAdviceProviderRequest {
        dispatch_id: Uuid::from_u128(9),
        request: tect_domain::ScopeAdviceRequest {
            contract: "fixture".into(),
            source_digest: "a".repeat(64),
            manifest_digest: "a".repeat(64),
            eligible_set_digest: "a".repeat(64),
            baseline_id: tect_domain::ScopeAlternativeId("a".repeat(64)),
            alternatives: Vec::new(),
            questions: Vec::new(),
            digest: "a".repeat(64),
        },
        budget_policy: crate::ScopeBudgetPolicyEvaluation {
            policy_id: verified_policy.id().to_string(),
        },
    };
    let prepared = provider
        .prepare_context(&crate::ScopeAdviceProviderContext::for_test(
            request.request.clone(),
        ))
        .unwrap();
    let expected_body = prepared.body().to_vec();
    let mut config = no_call_config(WorkspaceAdvisoryMode::Optional);
    config.provider_profile_ref = Some(AdvisoryProviderProfileRef {
        id: "fixture-profile".into(),
    });
    config.model_configuration = Some(AdvisoryModelConfiguration {
        model: "fixture-model".into(),
    });
    let authorization = scope_dispatch_authorization(
        &prepared,
        ScopeDispatchMetadata {
            dispatch_id: request.dispatch_id,
            opportunity_id: Uuid::from_u128(10),
            provider: "fixture",
            adapter_version: "v1",
            material_digest: "a".repeat(64),
        },
        &config,
        &request.budget_policy,
        &verified_policy,
    )
    .unwrap();
    assert_eq!(authorization.request_payload, expected_body);
    assert_eq!(authorization.payload_digest, sha256(&expected_body));
    let started = fixture_started_dispatch(&authorization, true);
    let permit =
        StartedScopeDispatchPermit::after_committed_start(&started, &authorization, &prepared)
            .unwrap();
    assert_eq!(
        provider
            .attempt_prepared(&request, prepared, permit)
            .await
            .unwrap(),
        observation
    );
    assert_eq!(
        *received_body.lock().unwrap(),
        Some(authorization.request_payload)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn permit_requires_send_start_and_binds_exact_prepared_entity() {
    let verified_policy = super::budget::syntactic_policy(65_536);
    let request = fixture_scope_request();
    let prepared = PreparedScopeAdviceAttempt::new(
        request.clone(),
        b"{\"scope\":\"private-marker\"}".to_vec(),
        "fixture-profile".into(),
        "fixture-model".into(),
        "https://fixture.invalid/advice".into(),
        "fixture-wire/1".into(),
    )
    .unwrap();
    let mut config = no_call_config(WorkspaceAdvisoryMode::Optional);
    config.provider_profile_ref = Some(AdvisoryProviderProfileRef {
        id: "fixture-profile".into(),
    });
    config.model_configuration = Some(AdvisoryModelConfiguration {
        model: "fixture-model".into(),
    });
    let authorization = scope_dispatch_authorization(
        &prepared,
        ScopeDispatchMetadata {
            dispatch_id: Uuid::from_u128(21),
            opportunity_id: Uuid::from_u128(22),
            provider: "fixture",
            adapter_version: "v1",
            material_digest: "a".repeat(64),
        },
        &config,
        &crate::ScopeBudgetPolicyEvaluation {
            policy_id: verified_policy.id().to_string(),
        },
        &verified_policy,
    )
    .unwrap();
    let mut started = fixture_started_dispatch(&authorization, false);
    assert!(
        StartedScopeDispatchPermit::after_committed_start(&started, &authorization, &prepared)
            .is_err()
    );
    started.should_send = true;
    started.dispatch.state = AdvisoryDispatchState::Authorized;
    assert!(
        StartedScopeDispatchPermit::after_committed_start(&started, &authorization, &prepared)
            .is_err()
    );
    started.dispatch.state = AdvisoryDispatchState::Sending;
    let permit =
        StartedScopeDispatchPermit::after_committed_start(&started, &authorization, &prepared)
            .unwrap();
    assert!(permit.permits(authorization.dispatch_id, &prepared));
    assert!(!permit.permits(Uuid::from_u128(23), &prepared));
    let changed_body = PreparedScopeAdviceAttempt::new(
        request,
        b"{\"scope\":\"different\"}".to_vec(),
        "fixture-profile".into(),
        "fixture-model".into(),
        "https://fixture.invalid/advice".into(),
        "fixture-wire/1".into(),
    )
    .unwrap();
    assert!(!permit.permits(authorization.dispatch_id, &changed_body));
    let rendered = format!("{prepared:?}");
    assert!(!rendered.contains("private-marker"));
    assert!(rendered.contains("[redacted]"));
}

#[test]
fn invalid_utf8_is_rejected_before_transport() {
    assert_eq!(
        PreparedScopeAdviceAttempt::new(
            fixture_scope_request(),
            vec![0xff],
            "profile".into(),
            "model".into(),
            "https://fixture.invalid/advice".into(),
            "wire/1".into(),
        ),
        Err(ScopeAdviceProviderError::ProvenNotSent)
    );
}

#[test]
fn permit_is_minted_only_after_successful_start_commit_in_runtime_path() {
    let source = ORCHESTRATION_SOURCE;
    let start = source
        .find(".start_signed_scope_dispatch(&lifecycle")
        .unwrap();
    let commit = source[start..].find("start.commit().await?").unwrap() + start;
    let no_send = source[commit..].find("if !started.should_send").unwrap() + commit;
    let mint = source[no_send..]
        .find("StartedScopeDispatchPermit::after_committed_start")
        .unwrap()
        + no_send;
    let send = source[mint..].find(".observe_prepared(").unwrap() + mint;
    assert!(start < commit && commit < no_send && no_send < mint && mint < send);
}
