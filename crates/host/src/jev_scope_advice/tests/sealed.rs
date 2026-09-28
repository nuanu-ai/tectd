use super::*;
use tect_application::{
    AdvisoryProviderReceiptObservation, AdvisoryProviderTransportContext,
    StoredAdvisoryProviderReceipt,
};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryDispatch, AdvisoryDispatchState,
    AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference,
    AdvisoryRetryBasis, CandidateBoundary, CandidateDelta, ResolvedCandidateDraft,
    ScopeDecompositionAlternative,
};

fn fixture() -> (
    JevScopeAdviceProvider,
    PreparedScopeAdviceAttempt,
    StoredAdvisoryProviderReceipt,
) {
    let provider = JevScopeAdviceProvider::new(
        JevScopeAdviceConfig {
            profile: "fixture".into(),
            endpoint: Url::parse("http://127.0.0.1:9/v1/systemone").unwrap(),
            model: "jev-1.13.0".into(),
            timeout: Duration::from_secs(1),
            maximum_request_bytes: 16384,
            maximum_response_bytes: 16384,
        },
        "fixture-only".into(),
    )
    .unwrap();
    let request = request();
    let emitted = vec![ScopeDecompositionAlternative {
        id: request.alternatives[0].id.clone(),
        kind: request.alternatives[0].kind,
        material_digest: request.alternatives[0].material_digest.clone(),
        coverage: vec![],
        material: ResolvedCandidateDraft {
            boundary: CandidateBoundary::Finite,
            goals: vec![],
            evidence: vec![],
            candidates: vec![],
            blockers: vec![],
            pending_question: None,
            empty_disposition: None,
            protected_changes: vec![],
            delta: CandidateDelta::default(),
        },
    }];
    let prepared = provider.prepare_with_emitted(&request, &emitted).unwrap();
    let id = Uuid::from_u128(1);
    let snapshot = json!({"provider_profile_ref":{"id":"fixture"},"model_configuration":{"model":"jev-1.13.0"},"adapter_version":"1","budget_policy_id":"fixture-policy","budget_policy":{"policy_id":"fixture-policy","policy_version":1,"policy_digest":"a".repeat(64)},"destination":prepared.destination(),"wire_version":prepared.wire_version(),"request_body_length":prepared.body_length(),"request_body_sha256":prepared.body_sha256()});
    let opportunity = AdvisoryOpportunity {
        id,
        workspace_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        authorized_actor_id: Uuid::from_u128(4),
        capability: AdvisoryCapability::ScopeDecomposition,
        decision_point: AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
        decision_point_version: 1,
        workflow_occurrence_key: "fixture".into(),
        target_kind: "scope_candidate_set".into(),
        target_id: Some(Uuid::from_u128(5)),
        work_revision: Some(1),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        matrix_verification_digest: None,
        source_ref: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        config_revision: 1,
        material_digest: "b".repeat(64),
        state: AdvisoryOpportunityState::AwaitingResponse,
        primary_reason: AdvisoryReason::DispatchAuthorized,
        provider_called: true,
    };
    let dispatch = AdvisoryDispatch {
        id: DISPATCH_ID,
        opportunity_id: id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        provider: "jev-system-one".into(),
        model: "jev-1.13.0".into(),
        configuration_digest: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&snapshot).unwrap())
        ),
        material_digest: opportunity.material_digest.clone(),
        payload_digest: prepared.body_sha256().into(),
        input_tokens: None,
        output_tokens: None,
        latency_ms: Some(7),
        state: AdvisoryDispatchState::Sealed,
        send_certainty: AdvisorySendCertainty::Sent,
        outcome: Some(AdvisoryDispatchOutcome::ProviderResponse),
        retry_basis: AdvisoryRetryBasis::Initial,
        raw_response_ref: None,
    };
    let saved = StoredAdvisoryProviderReceipt {
        opportunity,
        dispatch,
        configuration_snapshot: snapshot,
        request_payload: prepared.body().to_vec(),
        request_payload_sha256: prepared.body_sha256().into(),
        observation: Some(AdvisoryProviderReceiptObservation {
            response_payload: Some(serde_json::to_vec(&valid_response()).unwrap()),
            http_status: Some(200),
            input_tokens: None,
            output_tokens: None,
            response_complete: true,
            original_transport_context: Some(AdvisoryProviderTransportContext {
                send_certainty: AdvisorySendCertainty::Sent,
                outcome: AdvisoryDispatchOutcome::ProviderResponse,
                raw_response_ref: Some("fixture-raw".into()),
                provider_failure_code: None,
            }),
        }),
        original_elapsed_ms: Some(7),
    };
    (provider, prepared, saved)
}

#[test]
fn sealed_native_full_context_parses_without_mutation() {
    let (provider, prepared, saved) = fixture();
    let body: Value = serde_json::from_slice(prepared.body()).unwrap();
    let emitted: Vec<ScopeDecompositionAlternative> =
        serde_json::from_value(body["state"]["emitted"].clone()).unwrap();
    assert!(
        provider
            .prepare_with_emitted(&request(), &emitted)
            .is_ok_and(|current| current == prepared)
    );
    assert!(!provider.matches_legacy_prepared(&request(), &emitted, &prepared));
    let before = saved.observation.clone();
    let answers = provider.parse_sealed_response(&prepared, &saved).unwrap();
    assert_eq!(answers.answers.len(), 1);
    let usage = provider.usage_from_sealed_response(&saved).unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens),
        (Some(11), Some(5))
    );
    assert_eq!(saved.observation, before);
}

#[test]
fn sealed_v2_receipt_keeps_legacy_binary_choice_interpretation() {
    let (provider, prepared_v3, mut saved) = fixture();
    let body: Value = serde_json::from_slice(prepared_v3.body()).unwrap();
    let emitted: Vec<ScopeDecompositionAlternative> =
        serde_json::from_value(body["state"]["emitted"].clone()).unwrap();
    let v2_body = wire::serialize_request_v2("jev-1.13.0", &request(), &emitted).unwrap();
    let prepared = PreparedScopeAdviceAttempt::new(
        request(),
        v2_body,
        "fixture".into(),
        "jev-1.13.0".into(),
        prepared_v3.destination().into(),
        LEGACY_WIRE_FORMAT.into(),
    )
    .unwrap();
    saved.request_payload = prepared.body().to_vec();
    saved.request_payload_sha256 = prepared.body_sha256().into();
    saved.dispatch.payload_digest = prepared.body_sha256().into();
    saved.configuration_snapshot["wire_version"] = json!(LEGACY_WIRE_FORMAT);
    saved.configuration_snapshot["request_body_length"] = json!(prepared.body_length());
    saved.configuration_snapshot["request_body_sha256"] = json!(prepared.body_sha256());
    saved.dispatch.configuration_digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&saved.configuration_snapshot).unwrap())
    );
    saved.observation.as_mut().unwrap().response_payload = Some(serde_json::to_vec(&json!({
        "model":"jev-1.13.0", "answers": {
            format!("choice_{ID}"): {"type":"choice","choice":"PREFERRED","confidence":0.8,"probabilities":{"PREFERRED":0.8,"NON_PREFERRED":0.2}},
            format!("score_{ID}"): {"type":"score","score":2.4,"confidence":0.7,"legend":{"0":"conflict","1":"weak_fit","2":"fit","3":"strong_fit"},"probabilities":{"0":0.05,"1":0.1,"2":0.55,"3":0.3}}
        }, "usage":{"input_tokens":11,"output_tokens":5}
    })).unwrap());
    assert!(provider.matches_legacy_prepared(&request(), &emitted, &prepared));
    assert_ne!(
        provider.prepare_with_emitted(&request(), &emitted).unwrap(),
        prepared
    );
    let answers = provider.parse_sealed_response(&prepared, &saved).unwrap();
    assert_eq!(
        answers.answers[0].choice,
        tect_domain::ScopeAdviceChoice::Preferred
    );
    assert_eq!(answers.comparative_disposition, None);
    assert!(
        serde_json::to_value(&answers)
            .unwrap()
            .get("comparative_disposition")
            .is_none()
    );
    let legacy_normalized = format!(
        "{{\"answers\":[{{\"alternative_id\":\"{ID}\",\"choice\":\"preferred\",\"score\":\"fit\",\"choice_confidence\":8000,\"score_confidence\":7000}}]}}"
    );
    assert_eq!(
        serde_json::to_vec(&answers).unwrap(),
        legacy_normalized.as_bytes()
    );
    assert_eq!(
        provider
            .usage_from_sealed_response(&saved)
            .unwrap()
            .input_tokens,
        Some(11)
    );
}

#[test]
fn sealed_v2_receipt_rejects_self_consistent_mismatched_emitted_material() {
    let (provider, prepared_v3, mut saved) = fixture();
    let body: Value = serde_json::from_slice(prepared_v3.body()).unwrap();
    let emitted: Vec<ScopeDecompositionAlternative> =
        serde_json::from_value(body["state"]["emitted"].clone()).unwrap();
    let v2_body = wire::serialize_request_v2("jev-1.13.0", &request(), &emitted).unwrap();
    let mut malformed: Value = serde_json::from_slice(&v2_body).unwrap();
    malformed["state"]["emitted"][0]["material_digest"] = json!("0".repeat(64));
    let prepared = PreparedScopeAdviceAttempt::new(
        request(),
        serde_json::to_vec(&malformed).unwrap(),
        "fixture".into(),
        "jev-1.13.0".into(),
        prepared_v3.destination().into(),
        LEGACY_WIRE_FORMAT.into(),
    )
    .unwrap();
    saved.request_payload = prepared.body().to_vec();
    saved.request_payload_sha256 = prepared.body_sha256().into();
    saved.dispatch.payload_digest = prepared.body_sha256().into();
    saved.configuration_snapshot["wire_version"] = json!(LEGACY_WIRE_FORMAT);
    saved.configuration_snapshot["request_body_length"] = json!(prepared.body_length());
    saved.configuration_snapshot["request_body_sha256"] = json!(prepared.body_sha256());
    saved.dispatch.configuration_digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&saved.configuration_snapshot).unwrap())
    );
    assert!(!provider.matches_legacy_prepared(&request(), &emitted, &prepared));
    assert_eq!(
        provider.parse_sealed_response(&prepared, &saved),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn partial_status_identity_and_malformed_answers_never_parse() {
    let (provider, prepared, mut saved) = fixture();
    saved.observation.as_mut().unwrap().response_complete = false;
    assert!(provider.parse_sealed_response(&prepared, &saved).is_err());
    assert_eq!(
        provider.usage_from_sealed_response(&saved).unwrap(),
        AdvisoryProviderReceiptUsage::default()
    );
    saved.observation.as_mut().unwrap().response_complete = true;
    saved.observation.as_mut().unwrap().http_status = Some(500);
    assert!(provider.parse_sealed_response(&prepared, &saved).is_err());
    saved.observation.as_mut().unwrap().http_status = Some(200);
    saved.dispatch.model = "different".into();
    assert!(provider.parse_sealed_response(&prepared, &saved).is_err());
    saved.dispatch.model = "jev-1.13.0".into();
    saved.observation.as_mut().unwrap().response_payload = Some(b"{".to_vec());
    assert!(provider.parse_sealed_response(&prepared, &saved).is_err());
    assert_eq!(
        provider.usage_from_sealed_response(&saved).unwrap(),
        AdvisoryProviderReceiptUsage::default()
    );
}

#[test]
fn durable_complete_raw_receipt_provides_usage_before_dispatch_seal() {
    let (provider, _, mut saved) = fixture();
    saved.dispatch.state = AdvisoryDispatchState::Sending;
    saved.dispatch.send_certainty = AdvisorySendCertainty::SentUnknown;
    let raw = saved.observation.as_mut().unwrap();
    raw.response_payload = Some(br#"{"usage":{"input_tokens":20,"output_tokens":30}}"#.to_vec());
    let usage = provider.usage_from_sealed_response(&saved).unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens),
        (Some(20), Some(30))
    );
    saved.observation.as_mut().unwrap().response_complete = false;
    assert_eq!(
        provider.usage_from_sealed_response(&saved).unwrap(),
        AdvisoryProviderReceiptUsage::default()
    );
    saved.observation = None;
    assert_eq!(
        provider.usage_from_sealed_response(&saved).unwrap(),
        AdvisoryProviderReceiptUsage::default()
    );
}

#[test]
fn ambiguous_usage_stays_unknown_in_durable_sending_and_sealed_receipts() {
    for state in [
        AdvisoryDispatchState::Sending,
        AdvisoryDispatchState::Sealed,
    ] {
        let (provider, _, mut saved) = fixture();
        saved.dispatch.state = state;
        for body in [
            br#"{"usage":{"input_tokens":999999,"input_tokens":0,"output_tokens":1}}"#.as_slice(),
            br#"{"usage":{"input_tokens":999999,"output_tokens":1},"usage":{"input_tokens":0,"output_tokens":1}}"#.as_slice(),
        ] {
            saved.observation.as_mut().unwrap().response_payload = Some(body.to_vec());
            let before = saved.observation.clone();
            assert_eq!(provider.usage_from_sealed_response(&saved).unwrap(), AdvisoryProviderReceiptUsage::default());
            assert_eq!(saved.observation, before);
        }
        saved.observation.as_mut().unwrap().response_payload =
            Some(br#"{"usage":{"input_tokens":20,"output_tokens":30}}"#.to_vec());
        let usage = provider.usage_from_sealed_response(&saved).unwrap();
        assert_eq!(
            (usage.input_tokens, usage.output_tokens),
            (Some(20), Some(30))
        );
    }
}
