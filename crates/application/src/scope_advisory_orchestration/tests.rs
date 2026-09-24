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
        "supply_scope_manifest(",
        ".scope_budget",
        ".attempt_prepared(\n                &ScopeAdviceProviderRequest",
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

#[tokio::test]
async fn authored_supply_receives_authority_and_exact_request_without_legacy_fallback() {
    let candidate_set_id = Uuid::from_u128(5);
    let source = authored_source(candidate_set_id, 7);
    let observation = crate::ScopeAuthorityObservation {
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id,
        source: source.clone(),
        obligations: vec![SourceObligation {
            id: "obligation.intent".into(),
            source_input_id: "program.intent".into(),
            statement_digest: "a".repeat(64),
            conditions: vec![],
            exceptions: vec![],
        }],
    };
    let manifest = authored_manifest(source);
    let authored_calls = Arc::new(AtomicUsize::new(0));
    let legacy_calls = Arc::new(AtomicUsize::new(0));
    let seen_authored = Arc::new(std::sync::Mutex::new(None));
    let supplier = RecordingManifestSupplier {
        authored_calls: authored_calls.clone(),
        legacy_calls: legacy_calls.clone(),
        seen_authored: seen_authored.clone(),
        manifest: manifest.clone(),
        fail_authored: false,
    };
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };

    assert_eq!(
        supply_scope_manifest(&supplier, Uuid::from_u128(9), &observation, &request).await,
        Ok(manifest)
    );
    assert_eq!(authored_calls.load(Ordering::SeqCst), 1);
    assert_eq!(legacy_calls.load(Ordering::SeqCst), 0);
    let seen = seen_authored.lock().unwrap().clone().unwrap();
    assert_eq!(seen.tenant_id, Uuid::from_u128(9));
    assert_eq!(seen.observation, observation);
    assert_eq!(seen.authored_scope_set, request.authored_scope_set.unwrap());
}

#[tokio::test]
async fn authored_revision_mismatch_and_supplier_failure_fail_closed() {
    let candidate_set_id = Uuid::from_u128(5);
    let source = authored_source(candidate_set_id, 8);
    let observation = crate::ScopeAuthorityObservation {
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id,
        source: source.clone(),
        obligations: vec![SourceObligation {
            id: "obligation.intent".into(),
            source_input_id: "program.intent".into(),
            statement_digest: "a".repeat(64),
            conditions: vec![],
            exceptions: vec![],
        }],
    };
    let authored_calls = Arc::new(AtomicUsize::new(0));
    let legacy_calls = Arc::new(AtomicUsize::new(0));
    let supplier = RecordingManifestSupplier {
        authored_calls: authored_calls.clone(),
        legacy_calls: legacy_calls.clone(),
        seen_authored: Arc::new(std::sync::Mutex::new(None)),
        manifest: authored_manifest(source),
        fail_authored: false,
    };
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };
    assert_eq!(
        supply_scope_manifest(&supplier, Uuid::from_u128(9), &observation, &request).await,
        Err(Error::StaleRevision)
    );
    assert_eq!(authored_calls.load(Ordering::SeqCst), 0);

    let failing = RecordingManifestSupplier {
        authored_calls: authored_calls.clone(),
        legacy_calls: legacy_calls.clone(),
        seen_authored: supplier.seen_authored.clone(),
        manifest: supplier.manifest.clone(),
        fail_authored: true,
    };
    let mut matching_request = request;
    matching_request
        .authored_scope_set
        .as_mut()
        .unwrap()
        .expected_candidate_set_revision = 8;
    assert_eq!(
        supply_scope_manifest(
            &failing,
            Uuid::from_u128(9),
            &observation,
            &matching_request
        )
        .await,
        Err(Error::InputPending)
    );
    assert_eq!(authored_calls.load(Ordering::SeqCst), 1);
    assert_eq!(legacy_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn authored_manifest_replay_binds_request_digest_and_all_persisted_identity() {
    let candidate_set_id = Uuid::from_u128(5);
    let manifest = authored_manifest(authored_source(candidate_set_id, 7));
    let config = no_call_config(WorkspaceAdvisoryMode::Optional);
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };
    let digest = authored_request_digest(request.authored_scope_set.as_ref().unwrap()).unwrap();
    let opportunity = opportunity_for_authored_manifest(&request, &config, &manifest);
    let record = ScopeManifestRecord {
        opportunity_id: opportunity.id,
        candidate_set_id,
        config_revision: config.revision,
        opportunity_material_digest: manifest.whole_set_digest.clone(),
        manifest,
    };
    let stored = crate::StoredScopeManifestRecord {
        record,
        authored_request_digest: Some(digest.clone()),
    };
    assert!(
        validate_authored_replay_binding(
            &stored,
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
            &digest,
        )
        .is_ok()
    );
    assert_eq!(
        validate_authored_replay_binding(
            &stored,
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
            &"b".repeat(64),
        ),
        Err(Error::InputConflict)
    );
    let mut legacy = stored;
    legacy.authored_request_digest = None;
    assert_eq!(
        validate_authored_replay_binding(
            &legacy,
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
            &digest,
        ),
        Err(Error::InputConflict)
    );
}

#[test]
fn dispatch_stale_errors_map_only_to_their_audited_terminal_reasons() {
    assert_eq!(
        prepared_scope_stale_reason(&Error::StaleRevision),
        Some(AdvisoryReason::ConfigurationChanged)
    );
    assert_eq!(
        prepared_scope_stale_reason(&Error::StaleContext),
        Some(AdvisoryReason::DeterministicInputInvalid)
    );
    assert_eq!(prepared_scope_stale_reason(&Error::InputConflict), None);
    assert_eq!(prepared_scope_stale_reason(&Error::NotFound), None);

    let candidate_set_id = Uuid::from_u128(5);
    let manifest = authored_manifest(authored_source(candidate_set_id, 7));
    let config = no_call_config(WorkspaceAdvisoryMode::Optional);
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };
    let prepared = opportunity_for_authored_manifest(&request, &config, &manifest);
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(AdvisoryOpportunity {
            state: AdvisoryOpportunityState::Prepared,
            primary_reason: AdvisoryReason::DispatchAuthorized,
            ..prepared.clone()
        }),
        Err(Error::InputConflict)
    );
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(AdvisoryOpportunity {
            state: AdvisoryOpportunityState::Invalidated,
            primary_reason: AdvisoryReason::ConfigurationChanged,
            ..prepared.clone()
        })
        .unwrap()
        .state,
        AdvisoryOpportunityState::Invalidated
    );
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(AdvisoryOpportunity {
            state: AdvisoryOpportunityState::NoCall,
            primary_reason: AdvisoryReason::DeterministicInputInvalid,
            ..prepared.clone()
        })
        .unwrap()
        .primary_reason,
        AdvisoryReason::DeterministicInputInvalid
    );
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(AdvisoryOpportunity {
            state: AdvisoryOpportunityState::NoCall,
            primary_reason: AdvisoryReason::DeterministicInputInvalid,
            provider_called: true,
            ..prepared
        }),
        Err(Error::InputConflict)
    );
}

#[test]
fn authored_no_call_replay_conflicts_on_changed_request_payload() {
    let config = no_call_config(WorkspaceAdvisoryMode::Optional);
    let mut request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id: Uuid::from_u128(5),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };
    let reason = AdvisoryReason::CapabilityUnavailable;
    let opportunity = AdvisoryOpportunity {
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
        work_revision: Some(7),
        source_ref: None,
        session_preference: request.session_preference,
        request_preference: request.request_preference,
        config_revision: config.revision,
        material_digest: no_call_digest(&request, config.revision, reason).unwrap(),
        state: AdvisoryOpportunityState::NoCall,
        primary_reason: reason,
        provider_called: false,
    };
    assert!(
        validate_authored_no_call_replay(
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
        )
        .is_ok()
    );
    request.authored_scope_set.as_mut().unwrap().alternatives[0]
        .draft
        .empty_disposition
        .as_mut()
        .unwrap()
        .reason
        .push('!');
    assert_eq!(
        validate_authored_no_call_replay(
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
        ),
        Err(Error::InputConflict)
    );
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
    let source = include_str!("../scope_advisory_orchestration.rs");
    let budget = source.find(".scope_budget").unwrap();
    let no_call = source[budget..]
        .find("AdvisoryReason::BudgetPolicyInvalid")
        .unwrap();
    let provider = source[budget..]
        .find("prepare_scope_advice_attempt(")
        .unwrap();
    assert!(no_call < provider);
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

#[tokio::test]
async fn fixture_provider_returns_normalized_answers_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let received_body = Arc::new(Mutex::new(None));
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
        received_body: received_body.clone(),
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
        request.dispatch_id,
        Uuid::from_u128(10),
        "fixture",
        "v1",
        &config,
        "a".repeat(64),
        "owner:fixture",
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
        Uuid::from_u128(21),
        Uuid::from_u128(22),
        "fixture",
        "v1",
        &config,
        "a".repeat(64),
        "owner:fixture",
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
    let source = include_str!("../scope_advisory_orchestration.rs");
    let start = source.find(".start_advisory_dispatch(&lifecycle").unwrap();
    let commit = source[start..].find("start.commit().await?").unwrap() + start;
    let no_send = source[commit..].find("if !started.should_send").unwrap() + commit;
    let mint = source[no_send..]
        .find("StartedScopeDispatchPermit::after_committed_start")
        .unwrap()
        + no_send;
    let send = source[mint..].find(".attempt_prepared(").unwrap() + mint;
    assert!(start < commit && commit < no_send && no_send < mint && mint < send);
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

    let source = include_str!("../scope_advisory_orchestration.rs");
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
    let source = include_str!("../scope_advisory_orchestration.rs");
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
    let send = source.find(".attempt_prepared(").unwrap();
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
    let source = include_str!("../scope_advisory_orchestration.rs");
    assert!(!source.contains("scope_caller.call"));
    assert!(!source.contains("scope_verifier.verify"));
    assert_eq!(source.matches(".attempt_prepared(").count(), 1);
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
        .find("let manifest = match supply_scope_manifest(")
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
    let provider = source.find(".attempt_prepared(").unwrap();
    let sealed = source.find(".seal_advisory_dispatch").unwrap();
    let reobserved = source[sealed..].find("scope_authority.observe").unwrap() + sealed;
    let finalized = source.find(".finalize_guarded_scope_advice").unwrap();
    assert!(replay < policy && policy < provider);
    assert!(sealed < reobserved && reobserved < finalized);
    assert!(source.contains("invalidate_scope_advisory"));
    assert_eq!(source.matches("capture_invalid_scope_input").count(), 4);
}

#[test]
fn authored_lookup_replay_and_failure_paths_precede_external_attempts() {
    let source = include_str!("../scope_advisory_orchestration.rs");
    let digest = source.find(".map(authored_request_digest)").unwrap();
    let request_lookup = source
        .find("read.scope_advisory_manifest_by_request_key")
        .unwrap();
    let replay = source.find("replay_authored_scope_advisory(").unwrap();
    let observer = source
        .find("self.scope_authority.observe(&authority_request)")
        .unwrap();
    let supplier = source.find("supply_scope_manifest(").unwrap();
    assert!(digest < request_lookup && request_lookup < replay);
    let provider = source.find(".attempt_prepared(").unwrap();
    assert!(replay < observer && observer < supplier && replay < provider);
    let early_no_call = source.find("if let Some((reason, revision))").unwrap();
    let active_input_gate = source
        .find("if request.authored_scope_set.is_none()")
        .unwrap();
    assert!(early_no_call < active_input_gate && active_input_gate < observer);
    assert!(source[active_input_gate..observer].contains("return Err(Error::InputPending)"));

    let authored_persist = source
        .find(".prepare_authored_scope_advisory_manifest(")
        .unwrap();
    let persisted_commit = source[authored_persist..]
        .find("prepare.commit().await?")
        .unwrap()
        + authored_persist;
    let reobserved = source[persisted_commit..]
        .find("self.scope_authority.observe(&authority_request)")
        .unwrap()
        + persisted_commit;
    let no_call_transition = source[reobserved..]
        .find("finalize_prepared_scope_advisory_without_dispatch")
        .unwrap()
        + reobserved;
    let provider = source.find(".attempt_prepared(").unwrap();
    assert!(authored_persist < persisted_commit);
    assert!(persisted_commit < reobserved);
    assert!(reobserved < no_call_transition && no_call_transition < provider);
}

#[test]
fn authorize_staleness_and_cancelled_start_terminalize_before_provider_attempt() {
    let source = include_str!("../scope_advisory_orchestration.rs");
    let authorize = source
        .find(".authorize_advisory_dispatch(&lifecycle")
        .unwrap();
    let stale_mapping = source[authorize..]
        .find("prepared_scope_stale_reason(&error)")
        .unwrap()
        + authorize;
    let rollback = source[stale_mapping..].find("drop(authorize)").unwrap() + stale_mapping;
    let close = source[rollback..]
        .find(".finalize_prepared_scope_stale(")
        .unwrap()
        + rollback;
    let start = source.find(".start_advisory_dispatch(&lifecycle").unwrap();
    let cancelled = source[start..]
        .find("started.dispatch.state == AdvisoryDispatchState::Cancelled")
        .unwrap()
        + start;
    let terminal_load = source[cancelled..]
        .find("advisory_opportunity_for_dispatch(workspace.id, opportunity.id)")
        .unwrap()
        + cancelled;
    let provider = source.find(".attempt_prepared(").unwrap();
    assert!(authorize < stale_mapping && stale_mapping < rollback && rollback < close);
    assert!(close < start && start < cancelled && cancelled < terminal_load);
    assert!(terminal_load < provider);

    let helper = include_str!("helpers.rs");
    assert!(helper.contains("Error::StaleRevision => Some(AdvisoryReason::ConfigurationChanged)"));
    assert!(
        helper.contains("Error::StaleContext => Some(AdvisoryReason::DeterministicInputInvalid)")
    );
    assert!(helper.contains("if !expected || opportunity.provider_called"));
    assert!(source.contains("started.dispatch.opportunity_id == opportunity.id"));
    assert!(source.contains("started.dispatch.outcome.is_none()"));
    assert!(source.contains("started.dispatch.raw_response_ref.is_none()"));
}

#[test]
fn authored_supplier_failure_is_captured_as_no_call_before_budget_or_provider() {
    let source = include_str!("../scope_advisory_orchestration.rs");
    let supplied = source
        .find("let manifest = match supply_scope_manifest(")
        .unwrap();
    let failure = source[supplied..].find("Err(_) =>").unwrap() + supplied;
    let capture = source[failure..]
        .find("capture_invalid_scope_input")
        .unwrap()
        + failure;
    let budget = source.find(".scope_budget").unwrap();
    let provider = source.find(".attempt_prepared(").unwrap();
    assert!(supplied < failure && failure < capture);
    assert!(capture < budget && budget < provider);
}

#[test]
fn authored_request_entry_and_explicit_adapter_constructor_are_public_without_effects() {
    let root = include_str!("../lib.rs");
    let service = include_str!("../service.rs");
    let orchestration = include_str!("../scope_advisory_orchestration.rs");
    assert!(root.contains("ScopeAdviceProviderRequest"));
    assert!(!root.contains("pub use scope_advisory_runtime::*;"));
    assert!(service.contains("pub fn new_with_scope_advisory_adapters("));
    assert!(orchestration.contains("pub async fn run_scope_advisory"));
    assert!(root.contains("RunScopeAdvisory, ScopeAdvisoryOutcome"));
    assert!(!orchestration.contains("pub async fn decide_scope_advisory"));
    assert!(!orchestration.contains("pub async fn preserve_scope_advisory"));
}
