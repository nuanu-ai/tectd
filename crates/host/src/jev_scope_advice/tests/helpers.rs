use super::*;
use tect_application::{
    AdvisoryProviderReceiptObservation, AdvisoryProviderTransportContext,
    ScopeAdviceProviderContext, StoredAdvisoryProviderReceipt,
};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryDispatch, AdvisoryDispatchState,
    AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference,
    AdvisoryRetryBasis,
};

pub(super) fn native_context() -> ScopeAdviceProviderContext {
    let manifest = fixtures::fixture_manifest();
    manifest.validate(&fixtures::digest()).unwrap();
    assert!(!manifest.emitted.is_empty());
    let request = ScopeAdviceRequest::from_manifest(&fixtures::digest(), &manifest).unwrap();
    ScopeAdviceProviderContext::from_manifest(&request, &manifest).unwrap()
}

pub(super) fn native_request() -> ScopeAdviceRequest {
    native_context().request().clone()
}

pub(super) fn native_response(request: &ScopeAdviceRequest, abstain: bool) -> Value {
    let mut answers = serde_json::Map::new();
    let mut probabilities = serde_json::Map::new();
    let mut ids = request
        .alternatives
        .iter()
        .map(|a| a.id.clone())
        .collect::<Vec<_>>();
    ids.sort();
    for (index, id) in ids.iter().enumerate() {
        probabilities.insert(
            format!("C{index}"),
            json!(if !abstain && index == 0 { 0.8 } else { 0.0 }),
        );
        answers.insert(
            format!("score_{}", id.0),
            json!({
                "type":"score", "score":if index == 0 {2.4} else {1.4}, "confidence":0.7,
                "legend":{"0":"conflict","1":"weak_fit","2":"fit","3":"strong_fit"},
                "probabilities":{"0":0.05,"1":0.1,"2":0.55,"3":0.3}
            }),
        );
    }
    probabilities.insert("ABSTAIN".into(), json!(if abstain { 1.0 } else { 0.2 }));
    answers.insert(
        "choice_v3".into(),
        json!({
            "type":"choice","choice":if abstain {"ABSTAIN"} else {"C0"},
            "confidence":0.8,"probabilities":probabilities
        }),
    );
    json!({"model":"jev-1.13.0","answers":answers,"usage":{"input_tokens":11,"output_tokens":5}})
}

pub(super) fn synthetic_provider() -> JevScopeAdviceProvider {
    JevScopeAdviceProvider::new(
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
    .unwrap()
}

pub(super) fn raw_response(bytes: Vec<u8>) -> AdvisoryProviderReceiptObservation {
    AdvisoryProviderReceiptObservation {
        response_payload: Some(bytes),
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
    }
}

/// A unit-only already-durable-shaped value, not a PG write, authorization,
/// signature validation, or evidence that a live dispatch was sealed.
pub(super) fn synthetic_saved(
    prepared: &PreparedScopeAdviceAttempt,
    raw: AdvisoryProviderReceiptObservation,
) -> StoredAdvisoryProviderReceipt {
    let snapshot = json!({
        "provider_profile_ref":{"id":prepared.profile()},
        "model_configuration":{"model":prepared.model()},"adapter_version":"1",
        "budget_policy_id":"fixture-policy",
        "budget_policy":{"policy_id":"fixture-policy","policy_version":1,"policy_digest":"a".repeat(64)},
        "destination":prepared.destination(),"wire_version":prepared.wire_version(),
        "request_body_length":prepared.body_length(),"request_body_sha256":prepared.body_sha256()
    });
    assert_eq!(snapshot.as_object().unwrap().len(), 9);
    let id = Uuid::from_u128(1);
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
        material_digest: prepared.request().digest.clone(),
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
        model: prepared.model().into(),
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
    StoredAdvisoryProviderReceipt {
        opportunity,
        dispatch,
        configuration_snapshot: snapshot,
        request_payload: prepared.body().to_vec(),
        request_payload_sha256: prepared.body_sha256().into(),
        observation: Some(raw),
        original_elapsed_ms: Some(7),
    }
}

pub(super) fn saved_fixture() -> (
    JevScopeAdviceProvider,
    PreparedScopeAdviceAttempt,
    StoredAdvisoryProviderReceipt,
) {
    let provider = synthetic_provider();
    let context = native_context();
    let prepared = provider.prepare_context(&context).unwrap();
    let raw = raw_response(serde_json::to_vec(&native_response(context.request(), false)).unwrap());
    let saved = synthetic_saved(&prepared, raw);
    (provider, prepared, saved)
}

pub(super) fn legacy_response(request: &ScopeAdviceRequest) -> Value {
    let mut answers = serde_json::Map::new();
    for (index, alternative) in request.alternatives.iter().enumerate() {
        answers.insert(
            format!("choice_{}", alternative.id.0),
            json!({
                "type":"choice","choice":if index==0 {"PREFERRED"} else {"NON_PREFERRED"},
                "confidence":0.8,"probabilities":{"PREFERRED":0.8,"NON_PREFERRED":0.2}
            }),
        );
        answers.insert(
            format!("score_{}", alternative.id.0),
            json!({
                "type":"score","score":2.4,"confidence":0.7,
                "legend":{"0":"conflict","1":"weak_fit","2":"fit","3":"strong_fit"},
                "probabilities":{"0":0.05,"1":0.1,"2":0.55,"3":0.3}
            }),
        );
    }
    json!({"model":"jev-1.13.0","answers":answers,"usage":{"input_tokens":11,"output_tokens":5}})
}
