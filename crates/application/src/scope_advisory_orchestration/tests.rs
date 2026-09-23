use super::*;
use crate::{
    DenyScopeBudget, DisabledScopeAdviceProvider, ScopeAdviceProvider, ScopeAdviceProviderError,
    ScopeAdviceProviderObservation, ScopeAdviceProviderRequest, ScopeAuthorityObserver,
    ScopeAuthorityOutcome, ScopeAuthorizedInvalidObservation, ScopeBudgetPolicy,
    ScopeBudgetRequest, UnavailableScopeManifestSupplier,
};
use async_trait::async_trait;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisoryReason, AdvisoryRequestPreference, AdvisorySendCertainty,
    ConfidenceBasisPoints, FrozenScopeSource, NormalizedScopeAdviceAnswers, ScopeAdviceChoice,
    ScopeAdviceScoreBand,
};

#[test]
fn no_call_material_is_deterministic_and_binds_reason_and_revision() {
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(1),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let first = no_call_digest(&request, 3, AdvisoryReason::CapabilityUnavailable).unwrap();
    assert_eq!(
        first,
        no_call_digest(&request, 3, AdvisoryReason::CapabilityUnavailable).unwrap()
    );
    assert_ne!(
        first,
        no_call_digest(&request, 4, AdvisoryReason::CapabilityUnavailable).unwrap()
    );
    assert_ne!(
        first,
        no_call_digest(&request, 3, AdvisoryReason::ProviderUnconfigured).unwrap()
    );
}

#[tokio::test]
async fn production_defaults_fail_closed_without_supplier_budget_or_provider() {
    assert!(crate::ScopeManifestSupplier::identity(&UnavailableScopeManifestSupplier).is_none());
    let budget = DenyScopeBudget;
    assert_eq!(
        budget
            .evaluate(&ScopeBudgetRequest {
                workspace_id: Uuid::from_u128(1),
                actor_id: Uuid::from_u128(2),
                candidate_set_id: Uuid::from_u128(3),
                config_revision: 0,
                manifest_digest: "a".repeat(64),
            })
            .await
            .unwrap(),
        None
    );
    assert!(crate::ScopeAdviceProvider::identity(&DisabledScopeAdviceProvider).is_none());
}

struct FixtureProvider {
    calls: Arc<AtomicUsize>,
    observation: ScopeAdviceProviderObservation,
}

#[async_trait]
impl ScopeAdviceProvider for FixtureProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("fixture", "v1"))
    }
    async fn attempt(
        &self,
        _: &ScopeAdviceProviderRequest,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.observation.clone())
    }
}

#[tokio::test]
async fn fixture_provider_returns_normalized_answers_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observation = ScopeAdviceProviderObservation {
        send_certainty: AdvisorySendCertainty::Sent,
        outcome: AdvisoryDispatchOutcome::ProviderResponse,
        answers: Some(NormalizedScopeAdviceAnswers {
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
        observation: observation.clone(),
    };
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
            policy_id: "owner:fixture".into(),
        },
    };
    assert_eq!(provider.attempt(&request).await.unwrap(), observation);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
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
    let source = include_str!("../scope_advisory_orchestration.rs");
    assert!(!source.contains("scope_caller.call"));
    assert!(!source.contains("scope_verifier.verify"));
    assert_eq!(
        source
            .matches(".attempt(&ScopeAdviceProviderRequest")
            .count(),
        1
    );
    assert!(source.contains("latency_ms: provider_observation.latency_ms"));
}

#[test]
fn no_host_route_or_http_adapter_is_added() {
    let runtime = include_str!("../scope_advisory_runtime.rs");
    assert!(!runtime.contains("reqwest"));
    assert!(!runtime.contains("hyper::"));
    assert!(!runtime.contains(&["Jev", "Dto"].concat()));
}

fn source(candidate_set_id: Uuid) -> FrozenScopeSource {
    FrozenScopeSource {
        candidate_set_id,
        candidate_set_revision: 1,
        snapshot_id: Uuid::from_u128(20),
        input_cursor: 0,
        program_id: Uuid::from_u128(21),
        program_revision: 1,
        program_latest_input: 0,
        planning_latest_input: 0,
        selected_sources_digest: "a".repeat(64),
        method_revision: "1".into(),
        method_digest: "a".repeat(64),
        registry_revision: "1".into(),
        registry_digest: "a".repeat(64),
        inputs: Vec::new(),
        digest: "a".repeat(64),
    }
}

#[test]
fn authority_binding_requires_candidate_set_identity() {
    let request = crate::ScopeAuthorityRequest {
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id: Uuid::from_u128(5),
    };
    let observation = crate::ScopeAuthorityObservation {
        workspace_id: request.workspace_id,
        actor_id: request.actor_id,
        session_id: request.session_id,
        candidate_set_id: request.candidate_set_id,
        source: source(request.candidate_set_id),
        obligations: Vec::new(),
    };
    assert!(validate_observation(&request, &observation).is_ok());
    let mut wrong_observation = observation.clone();
    wrong_observation.candidate_set_id = Uuid::from_u128(4);
    assert_eq!(
        validate_observation(&request, &wrong_observation),
        Err(tect_domain::Error::InputConflict)
    );
    let mut wrong_candidate = observation;
    wrong_candidate.source.candidate_set_id = Uuid::from_u128(4);
    assert_eq!(
        validate_observation(&request, &wrong_candidate),
        Err(tect_domain::Error::InputConflict)
    );
}

struct UnauthorizedObserver;

#[async_trait]
impl ScopeAuthorityObserver for UnauthorizedObserver {
    async fn observe(
        &self,
        _: &crate::ScopeAuthorityRequest,
    ) -> tect_domain::Result<ScopeAuthorityOutcome> {
        Err(tect_domain::Error::Unauthorized)
    }
}

#[tokio::test]
async fn unauthorized_observer_error_has_no_registered_decision_point() {
    let request = crate::ScopeAuthorityRequest {
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id: Uuid::from_u128(5),
    };
    assert_eq!(
        UnauthorizedObserver.observe(&request).await,
        Err(tect_domain::Error::Unauthorized)
    );
    let orchestration = include_str!("../scope_advisory_orchestration.rs");
    assert!(orchestration.contains("scope_authority.observe(&authority_request).await?"));
}

#[test]
fn authorized_invalid_observation_routes_to_durable_invalid_capture_before_policy() {
    let request = crate::ScopeAuthorityRequest {
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id: Uuid::from_u128(5),
    };
    let invalid = ScopeAuthorizedInvalidObservation {
        workspace_id: request.workspace_id,
        actor_id: request.actor_id,
        session_id: request.session_id,
        candidate_set_id: request.candidate_set_id,
    };
    assert!(validate_invalid_observation(&request, &invalid).is_ok());
    let orchestration = include_str!("../scope_advisory_orchestration.rs");
    let invalid_arm = orchestration.find("AuthorizedInvalid(value)").unwrap();
    let capture = orchestration[invalid_arm..]
        .find("capture_invalid_scope_input")
        .unwrap()
        + invalid_arm;
    let policy = orchestration
        .find("let preliminary = assess_advisory_policy")
        .unwrap();
    assert!(invalid_arm < capture && capture < policy);
    let missing_supplier = orchestration
        .find("scope_manifest_supplier.identity().is_none()")
        .unwrap();
    assert!(missing_supplier < policy);
    assert!(orchestration[missing_supplier..policy].contains("capture_invalid_scope_input"));
    let supplied_manifest = orchestration
        .find("let manifest = match self.scope_manifest_supplier.supply")
        .unwrap();
    assert!(supplied_manifest < policy);
    assert!(orchestration[supplied_manifest..policy].contains("capture_invalid_scope_input"));
    let capture_source = include_str!("capture.rs");
    assert!(capture_source.contains("deterministic_input_valid: false"));
    assert!(capture_source.contains("tx.commit().await?"));
}

#[test]
fn replay_precedes_policy_and_provider_and_success_is_rechecked_atomically() {
    let source = include_str!("../scope_advisory_orchestration.rs");
    let replay = source.find("advisory_opportunity_by_request").unwrap();
    let policy = source.find(".scope_budget").unwrap();
    let provider = source.find(".attempt(&ScopeAdviceProviderRequest").unwrap();
    let sealed = source.find(".seal_advisory_dispatch").unwrap();
    let reobserved = source[sealed..].find("scope_authority.observe").unwrap() + sealed;
    let finalized = source.find(".finalize_guarded_scope_advice").unwrap();
    assert!(replay < policy && policy < provider);
    assert!(sealed < reobserved && reobserved < finalized);
    assert!(source.contains("invalidate_scope_advisory"));
    assert_eq!(source.matches("capture_invalid_scope_input").count(), 4);
}

#[test]
fn provider_port_is_public_but_dispatch_entrypoints_and_injection_are_not() {
    let root = include_str!("../lib.rs");
    let service = include_str!("../service.rs");
    let orchestration = include_str!("../scope_advisory_orchestration.rs");
    assert!(root.contains("ScopeAdviceProviderRequest"));
    assert!(!root.contains("pub use scope_advisory_runtime::*;"));
    assert!(service.contains("#[cfg(test)]\n    pub(crate) fn with_scope_advisory_adapters"));
    assert!(!orchestration.contains("pub async fn run_scope_advisory"));
    assert!(!orchestration.contains("pub async fn decide_scope_advisory"));
    assert!(!orchestration.contains("pub async fn preserve_scope_advisory"));
}
