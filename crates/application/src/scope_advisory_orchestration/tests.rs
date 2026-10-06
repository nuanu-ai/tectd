use super::*;
use crate::{
    AuthoredScopeAlternative, DenyScopeBudget, DisabledScopeAdviceProvider,
    PreparedScopeAdviceAttempt, ScopeAdviceProvider, ScopeAdviceProviderError,
    ScopeAdviceProviderObservation, ScopeAdviceProviderRequest, ScopeAuthoredManifestRequest,
    ScopeAuthorityObserver, ScopeAuthorityOutcome, ScopeAuthorizedInvalidObservation,
    ScopeBudgetPolicy, ScopeBudgetRequest, ScopeManifestSupplier, UnavailableScopeManifestSupplier,
};
use async_trait::async_trait;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryDispatchOutcome, AdvisoryModelConfiguration,
    AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryProviderProfileRef, AdvisoryReason,
    AdvisoryRequestPreference, AdvisorySendCertainty, BuildSourceAuthoredScopeManifest,
    ConfidenceBasisPoints, EmptyCandidateDisposition, EmptyCandidateDispositionKind,
    FrozenScopeSource, FrozenSourceInput, NormalizedScopeAdviceAnswers, ObligationCoverage,
    ResolvedCandidateDraft, ScopeAdviceChoice, ScopeAdviceScoreBand, ScopeConstructorIdentity,
    ScopeDecompositionKind, SourceApplicability, SourceAuthoredScopeAlternative, SourceObligation,
    WorkspaceAdvisoryConfig, WorkspaceAdvisoryMode,
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

struct RecordingManifestSupplier {
    authored_calls: Arc<AtomicUsize>,
    legacy_calls: Arc<AtomicUsize>,
    seen_authored: Arc<std::sync::Mutex<Option<ScopeAuthoredManifestRequest>>>,
    manifest: tect_domain::ScopeConstructorManifest,
    fail_authored: bool,
}

#[async_trait]
impl ScopeManifestSupplier for RecordingManifestSupplier {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("fixture", "authored-v1"))
    }

    async fn supply(
        &self,
        _: &crate::ScopeAuthorityObservation,
    ) -> tect_domain::Result<tect_domain::ScopeConstructorManifest> {
        self.legacy_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.manifest.clone())
    }

    async fn supply_authored(
        &self,
        request: &ScopeAuthoredManifestRequest,
    ) -> tect_domain::Result<tect_domain::ScopeConstructorManifest> {
        self.authored_calls.fetch_add(1, Ordering::SeqCst);
        *self.seen_authored.lock().unwrap() = Some(request.clone());
        if self.fail_authored {
            return Err(Error::InputPending);
        }
        Ok(self.manifest.clone())
    }
}

fn authored_source(candidate_set_id: Uuid, revision: i64) -> FrozenScopeSource {
    let digest = "a".repeat(64);
    let mut source = FrozenScopeSource {
        candidate_set_id,
        candidate_set_revision: revision,
        snapshot_id: Uuid::from_u128(20),
        input_cursor: 0,
        program_id: Uuid::from_u128(21),
        program_revision: 1,
        program_latest_input: 0,
        planning_latest_input: 0,
        selected_sources_digest: digest.clone(),
        method_revision: "1".into(),
        method_digest: digest.clone(),
        registry_revision: "1".into(),
        registry_digest: digest.clone(),
        inputs: vec![FrozenSourceInput {
            id: "program.intent".into(),
            version: "1".into(),
            digest: digest.clone(),
            provenance: "program.intent@1".into(),
            applicability: SourceApplicability::Applicable,
        }],
        digest: String::new(),
    };
    source.digest = source.canonical_digest(&Sha256ScopeDigest).unwrap();
    source
}

fn authored_manifest(source: FrozenScopeSource) -> tect_domain::ScopeConstructorManifest {
    let digest = "a".repeat(64);
    tect_domain::build_source_authored_scope_manifest(
        &Sha256ScopeDigest,
        BuildSourceAuthoredScopeManifest {
            constructor: ScopeConstructorIdentity {
                id: "fixture-constructor".into(),
                version: "1".into(),
                digest: digest.clone(),
            },
            source,
            obligations: vec![SourceObligation {
                id: "obligation.intent".into(),
                source_input_id: "program.intent".into(),
                statement_digest: digest,
                conditions: vec![],
                exceptions: vec![],
            }],
            alternatives: vec![SourceAuthoredScopeAlternative {
                key: "baseline".into(),
                kind: ScopeDecompositionKind::Cohesive,
                material: ResolvedCandidateDraft {
                    boundary: tect_domain::CandidateBoundary::Ongoing,
                    goals: vec![],
                    evidence: vec![],
                    candidates: vec![],
                    blockers: vec![],
                    pending_question: None,
                    empty_disposition: Some(EmptyCandidateDisposition {
                        kind: EmptyCandidateDispositionKind::OutOfBoundary,
                        reason: "No in-boundary work".into(),
                        source_ref_id: Uuid::from_u128(22),
                    }),
                    protected_changes: vec![],
                    delta: Default::default(),
                },
                coverage: vec![ObligationCoverage {
                    obligation_id: "obligation.intent".into(),
                    condition_ids: vec![],
                    exception_ids: vec![],
                }],
            }],
            baseline_key: "baseline".into(),
        },
    )
    .unwrap()
}

#[test]
fn provider_context_preserves_bound_request_and_only_emitted_material() {
    let manifest = authored_manifest(authored_source(Uuid::from_u128(9), 7));
    let request =
        tect_domain::ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest).unwrap();
    let before = serde_json::to_vec(&request).unwrap();
    let context = crate::ScopeAdviceProviderContext::from_manifest(&request, &manifest).unwrap();
    assert_eq!(context.emitted(), manifest.emitted.as_slice());
    assert_eq!(context.request(), &request);
    assert_eq!(serde_json::to_vec(context.request()).unwrap(), before);
    assert_eq!(context.request().digest, request.digest);

    let mut changed_request = request.clone();
    changed_request.alternatives[0].material_digest = "f".repeat(64);
    assert!(crate::ScopeAdviceProviderContext::from_manifest(&changed_request, &manifest).is_err());
}

fn opportunity_for_authored_manifest(
    request: &RunScopeAdvisory,
    config: &WorkspaceAdvisoryConfig,
    manifest: &tect_domain::ScopeConstructorManifest,
) -> AdvisoryOpportunity {
    AdvisoryOpportunity {
        id: Uuid::from_u128(40),
        workspace_id: config.workspace_id,
        session_id: Uuid::from_u128(41),
        authorized_actor_id: Uuid::from_u128(42),
        capability: AdvisoryCapability::ScopeDecomposition,
        decision_point: AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
        decision_point_version: 1,
        workflow_occurrence_key: request.request_id.to_string(),
        target_kind: "scope_candidate_set".into(),
        target_id: Some(request.candidate_set_id),
        work_revision: Some(manifest.source.candidate_set_revision),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        matrix_verification_digest: None,
        source_ref: None,
        session_preference: request.session_preference,
        request_preference: request.request_preference,
        config_revision: config.revision,
        material_digest: manifest.whole_set_digest.clone(),
        state: AdvisoryOpportunityState::Prepared,
        primary_reason: AdvisoryReason::DispatchAuthorized,
        provider_called: false,
    }
}

struct FixtureProvider {
    calls: Arc<AtomicUsize>,
    received_body: Arc<Mutex<Option<Vec<u8>>>>,
    observation: ScopeAdviceProviderObservation,
}

fn fixture_started_dispatch(
    authorization: &AdvisoryDispatchAuthorization,
    should_send: bool,
) -> AdvisoryDispatchStart {
    AdvisoryDispatchStart {
        dispatch: tect_domain::AdvisoryDispatch {
            id: authorization.dispatch_id,
            opportunity_id: authorization.opportunity_id,
            predecessor_dispatch_id: None,
            attempt_number: 1,
            provider: authorization.provider.clone(),
            model: authorization.model.clone(),
            configuration_digest: authorization.configuration_digest.clone(),
            material_digest: authorization.material_digest.clone(),
            payload_digest: authorization.payload_digest.clone(),
            input_tokens: None,
            output_tokens: None,
            latency_ms: None,
            state: AdvisoryDispatchState::Sending,
            send_certainty: AdvisorySendCertainty::SentUnknown,
            outcome: None,
            retry_basis: tect_domain::AdvisoryRetryBasis::Initial,
            raw_response_ref: None,
        },
        should_send,
        // Synthetic constructor evidence only, not a persisted PG reservation.
        budget_reservation: Some(tect_domain::AdvisoryBudgetReservation {
            dispatch_id: authorization.dispatch_id,
            policy_id: authorization.configuration_snapshot["budget_policy"]["policy_id"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            policy_version: authorization.configuration_snapshot["budget_policy"]["policy_version"]
                .as_i64()
                .unwrap(),
            policy_digest: authorization.configuration_snapshot["budget_policy"]["policy_digest"]
                .as_str()
                .unwrap()
                .to_owned(),
            policy_effective_from_unix_ms: 1,
            policy_effective_until_unix_ms: 2,
            request_sha256: authorization.payload_digest.clone(),
            request_utf8_bytes: i64::try_from(authorization.request_payload.len()).unwrap(),
            reserved_calls: 1,
            reserved_retry_dispatches: 0,
            remaining_elapsed_ms: 1,
            reserved_input_tokens: 1,
            reserved_output_tokens: 1,
        }),
    }
}

#[async_trait]
impl ScopeAdviceProvider for FixtureProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("fixture", "v1"))
    }
    fn prepare_context(
        &self,
        context: &crate::ScopeAdviceProviderContext,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        let request = context.request();
        PreparedScopeAdviceAttempt::new(
            request.clone(),
            serde_json::to_vec(request).unwrap(),
            "fixture-profile".into(),
            "fixture-model".into(),
            "https://fixture.invalid/advice".into(),
            "fixture-wire/1".into(),
        )
    }
    async fn attempt_prepared(
        &self,
        request: &ScopeAdviceProviderRequest,
        prepared: PreparedScopeAdviceAttempt,
        permit: StartedScopeDispatchPermit,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        assert_eq!(prepared.request(), &request.request);
        assert!(permit.permits(request.dispatch_id, &prepared));
        *self.received_body.lock().unwrap() = Some(prepared.body().to_vec());
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.observation.clone())
    }
}

struct PreflightFixtureProvider {
    profile: String,
    model: String,
    maximum_body_bytes: usize,
    prepare_calls: Arc<AtomicUsize>,
    attempt_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ScopeAdviceProvider for PreflightFixtureProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("fixture", "v1"))
    }

    fn prepare_context(
        &self,
        context: &crate::ScopeAdviceProviderContext,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        let request = context.request();
        self.prepare_calls.fetch_add(1, Ordering::SeqCst);
        let body = serde_json::to_vec(request).unwrap();
        if body.len() > self.maximum_body_bytes {
            return Err(ScopeAdviceProviderError::ProvenNotSent);
        }
        PreparedScopeAdviceAttempt::new(
            request.clone(),
            body,
            self.profile.clone(),
            self.model.clone(),
            "https://fixture.invalid/advice".into(),
            "fixture-wire/1".into(),
        )
    }

    async fn attempt_prepared(
        &self,
        _: &ScopeAdviceProviderRequest,
        _: PreparedScopeAdviceAttempt,
        _: StartedScopeDispatchPermit,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        self.attempt_calls.fetch_add(1, Ordering::SeqCst);
        Err(ScopeAdviceProviderError::ProvenNotSent)
    }
}

fn fixture_scope_request() -> tect_domain::ScopeAdviceRequest {
    tect_domain::ScopeAdviceRequest {
        contract: "fixture".into(),
        source_digest: "a".repeat(64),
        manifest_digest: "a".repeat(64),
        eligible_set_digest: "a".repeat(64),
        baseline_id: tect_domain::ScopeAlternativeId("a".repeat(64)),
        alternatives: Vec::new(),
        questions: Vec::new(),
        digest: "a".repeat(64),
    }
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

const ORCHESTRATION_SOURCE: &str = concat!(
    include_str!("../scope_advisory_orchestration.rs"),
    include_str!("prepare.rs"),
    include_str!("dispatch.rs"),
    include_str!("receipts.rs"),
    include_str!("recovery.rs"),
);

mod authored;
mod authority;
mod budget;
mod contracts;
mod optionality;
mod preflight;
mod provider;

mod signed_permit;

mod session_preference;
