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
    ScopeAdviceScoreBand, WorkspaceAdvisoryConfig, WorkspaceAdvisoryMode,
};

fn no_call_config(mode: WorkspaceAdvisoryMode) -> WorkspaceAdvisoryConfig {
    WorkspaceAdvisoryConfig {
        workspace_id: Uuid::from_u128(1),
        revision: 3,
        mode,
        materialized: true,
        provider_profile_ref: None,
        model_configuration: None,
    }
}

struct FakeEarlyCandidateRevision {
    calls: usize,
    revision: Option<i64>,
}

#[async_trait]
impl EarlyCandidateRevision for FakeEarlyCandidateRevision {
    async fn revision(&mut self, _: Uuid, _: Uuid) -> tect_domain::Result<Option<i64>> {
        self.calls += 1;
        Ok(self.revision)
    }
}

#[tokio::test]
async fn early_no_call_uses_only_target_port_and_requires_target_access() {
    let workspace = Uuid::from_u128(1);
    let mut request = RunScopeAdvisory {
        request_id: Uuid::from_u128(2),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: None,
    };
    let mut target = FakeEarlyCandidateRevision {
        calls: 0,
        revision: Some(7),
    };
    assert_eq!(
        early_no_call_target(
            &mut target,
            workspace,
            &no_call_config(WorkspaceAdvisoryMode::Optional),
            &request,
        )
        .await,
        Ok(None)
    );
    assert_eq!(target.calls, 0);
    request.request_preference = AdvisoryRequestPreference::Skip;
    assert_eq!(
        early_no_call_target(
            &mut target,
            workspace,
            &no_call_config(WorkspaceAdvisoryMode::Optional),
            &request,
        )
        .await,
        Ok(Some((AdvisoryReason::RequestSkip, 7)))
    );
    assert_eq!(target.calls, 1);
    target.revision = None;
    assert_eq!(
        early_no_call_target(
            &mut target,
            workspace,
            &no_call_config(WorkspaceAdvisoryMode::Disabled),
            &request,
        )
        .await,
        Err(tect_domain::Error::NotFound)
    );
    assert_eq!(target.calls, 2);
}

#[test]
fn early_no_call_gate_preserves_disabled_session_request_precedence() {
    let mut request = RunScopeAdvisory {
        request_id: Uuid::from_u128(2),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::Skip,
        request_preference: AdvisoryRequestPreference::Skip,
        authored_scope_set: None,
    };
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Disabled), &request),
        Some(AdvisoryReason::WorkspaceDisabled)
    );
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Optional), &request),
        Some(AdvisoryReason::SessionSkip)
    );
    request.session_preference = AdvisoryRequestPreference::UseWorkspace;
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Optional), &request),
        Some(AdvisoryReason::RequestSkip)
    );
    request.request_preference = AdvisoryRequestPreference::UseWorkspace;
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Optional), &request),
        None
    );
}

#[test]
fn early_no_call_branch_precedes_all_external_advisory_ports() {
    let source = include_str!("../scope_advisory_orchestration.rs");
    let branch = source.find("if let Some((reason, revision))").unwrap();
    let return_from_branch = source[branch..]
        .find("let authority_request = ScopeAuthorityRequest")
        .unwrap()
        + branch;
    assert!(source[branch..return_from_branch].contains("capture_early_scope_no_call"));
    assert!(source[branch..return_from_branch].contains("return Ok(ScopeAdvisoryOutcome"));
    for port in [
        "scope_authority.observe(&authority_request)",
        "scope_manifest_supplier.supply(&observation)",
        ".scope_budget",
        ".attempt(&ScopeAdviceProviderRequest",
    ] {
        assert!(return_from_branch < source.find(port).unwrap(), "{port}");
    }
    assert!(
        source[..branch]
            .contains("early_no_call_target(&mut *read, workspace.id, &config, request)")
    );
    let capture = include_str!("capture.rs");
    let recheck = capture
        .find("pub(super) async fn capture_early_scope_no_call")
        .unwrap();
    let end = capture[recheck..]
        .find("pub(super) async fn capture_scope_opportunity")
        .unwrap()
        + recheck;
    assert!(
        capture[recheck..end]
            .contains("lock_candidate_revision(workspace.id, request.candidate_set_id)")
    );
    assert!(capture[recheck..end].contains("tx.commit().await?"));
}

#[test]
fn no_call_material_is_deterministic_and_binds_reason_and_revision() {
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(1),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: None,
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

fn authored_set() -> AuthoredScopeSet {
    AuthoredScopeSet {
        expected_candidate_set_revision: 7,
        baseline_key: "baseline".into(),
        alternatives: vec![AuthoredScopeAlternative {
            key: "baseline".into(),
            kind: tect_domain::ScopeDecompositionKind::Cohesive,
            draft: tect_domain::ScopeCandidateDraft {
                boundary: tect_domain::CandidateBoundary::Ongoing,
                goals: vec![],
                evidence: vec![],
                candidates: vec![],
                blockers: vec![],
                pending_question: None,
                empty_disposition: Some(tect_domain::EmptyCandidateDisposition {
                    kind: tect_domain::EmptyCandidateDispositionKind::OutOfBoundary,
                    reason: "No in-boundary work".into(),
                    source_ref_id: Uuid::from_u128(10),
                }),
                protected_changes: vec![],
                supersessions: vec![],
            },
            covered_source_ref_ids: vec![Uuid::from_u128(10), Uuid::from_u128(11)],
        }],
    }
}

#[test]
fn authored_contract_rejects_missing_baseline_duplicate_keys_and_unsorted_coverage() {
    let mut authored = authored_set();
    authored.validate().unwrap();
    authored.baseline_key = "missing".into();
    assert_eq!(authored.validate(), Err(Error::InvalidArguments));
    authored.baseline_key = "baseline".into();
    authored.alternatives.push(authored.alternatives[0].clone());
    assert_eq!(authored.validate(), Err(Error::InvalidArguments));
    authored.alternatives.pop();
    authored.alternatives[0].covered_source_ref_ids.reverse();
    assert_eq!(authored.validate(), Err(Error::InvalidArguments));
}

#[test]
fn authored_request_digest_changes_no_call_material_and_rejects_unknown_fields() {
    let mut request = RunScopeAdvisory {
        request_id: Uuid::from_u128(1),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::Skip,
        authored_scope_set: None,
    };
    let absent = no_call_digest(&request, 3, AdvisoryReason::RequestSkip).unwrap();
    request.authored_scope_set = Some(authored_set());
    let first = no_call_digest(&request, 3, AdvisoryReason::RequestSkip).unwrap();
    assert_ne!(absent, first);
    assert_eq!(
        first,
        no_call_digest(&request, 3, AdvisoryReason::RequestSkip).unwrap()
    );
    request.authored_scope_set.as_mut().unwrap().alternatives[0]
        .draft
        .empty_disposition
        .as_mut()
        .unwrap()
        .reason
        .push('!');
    assert_ne!(
        first,
        no_call_digest(&request, 3, AdvisoryReason::RequestSkip).unwrap()
    );

    let mut value = serde_json::to_value(authored_set()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AuthoredScopeSet>(value).is_err());
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
        tenant_id: Uuid::from_u128(9),
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
        tenant_id: Uuid::from_u128(9),
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
        tenant_id: Uuid::from_u128(9),
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
