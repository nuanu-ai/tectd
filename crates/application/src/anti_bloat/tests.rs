use super::*;
use crate::AntiBloatVerificationEvidence;
use crate::{AntiBloatSendPermit, StoredAntiBloatReview};
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tect_domain::*;

const D: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn input(extra: bool) -> AntiBloatInput {
    let digest = Sha256ScopeDigest;
    let mut source = FrozenScopeSource {
        candidate_set_id: Uuid::from_u128(1),
        candidate_set_revision: 3,
        snapshot_id: Uuid::from_u128(2),
        input_cursor: 2,
        program_id: Uuid::from_u128(3),
        program_revision: 4,
        program_latest_input: 2,
        planning_latest_input: 2,
        selected_sources_digest: D.into(),
        method_revision: "4".into(),
        method_digest: D.into(),
        registry_revision: "3".into(),
        registry_digest: D.into(),
        inputs: vec![FrozenSourceInput {
            id: Uuid::from_u128(50).to_string(),
            version: "4".into(),
            digest: D.into(),
            provenance: "program.intent@4".into(),
            applicability: SourceApplicability::Applicable,
        }],
        digest: String::new(),
    };
    source.digest = source.canonical_digest(&digest).unwrap();
    let constructor = ScopeConstructorIdentity {
        id: "fixture-constructor".into(),
        version: "1".into(),
        digest: D.into(),
    };
    let coverage = vec![ObligationCoverage {
        obligation_id: Uuid::from_u128(50).to_string(),
        condition_ids: vec!["condition.a".into()],
        exception_ids: vec!["exception.a".into()],
    }];
    let required = Uuid::from_u128(52);
    let required_goal = Uuid::from_u128(51);
    let mut material = ResolvedCandidateDraft {
        boundary: CandidateBoundary::Finite,
        goals: vec![CoverageGoalEntity {
            id: required_goal,
            revision: 1,
            text: "Preserve source".into(),
            source_ref_id: Uuid::from_u128(50),
            exact_quote: None,
            resolution: CoverageResolutionEntity {
                kind: CoverageResolutionKind::Candidate,
                id: required,
            },
        }],
        evidence: vec![],
        candidates: vec![CandidateEntity {
            id: required,
            revision: 1,
            grounding: CandidateGrounding::SourceGrounded,
            title: "Required".into(),
            outcome: "Required".into(),
            trigger: "Source".into(),
            delivered_behavior: "Deliver requirement".into(),
            proof: "Test".into(),
            includes: vec!["supplied".into()],
            excludes: vec![],
            dependencies: vec![],
            coverage_goal_ids: vec![required_goal],
            evidence_ids: vec![],
        }],
        blockers: vec![],
        pending_question: None,
        empty_disposition: None,
        protected_changes: vec![],
        delta: CandidateDelta {
            added: vec![CandidateAdded {
                candidate_id: required,
                revision: 1,
            }],
            ..CandidateDelta::default()
        },
    };
    if extra {
        let extra_id = Uuid::from_u128(70);
        let extra_goal = Uuid::from_u128(71);
        let mut candidate = material.candidates[0].clone();
        candidate.id = extra_id;
        candidate.title = "Optional dashboard".into();
        candidate.outcome = "Optional dashboard".into();
        candidate.delivered_behavior = "Show optional dashboard".into();
        candidate.coverage_goal_ids = vec![extra_goal];
        material.candidates.push(candidate);
        let mut goal = material.goals[0].clone();
        goal.id = extra_goal;
        goal.text = "Optional dashboard".into();
        goal.resolution.id = extra_id;
        material.goals.push(goal);
        material.delta.added.push(CandidateAdded {
            candidate_id: extra_id,
            revision: 1,
        });
    }
    let material_digest = scope_candidate_material_digest(&digest, &material).unwrap();
    let id = stable_scope_alternative_id(
        &digest,
        &constructor,
        &source.digest,
        ScopeDecompositionKind::Cohesive,
        &material_digest,
        &coverage,
    )
    .unwrap();
    let alternative = ScopeDecompositionAlternative {
        id: id.clone(),
        kind: ScopeDecompositionKind::Cohesive,
        material,
        material_digest,
        coverage,
    };
    let mut manifest = ScopeConstructorManifest {
        constructor,
        source,
        obligations: vec![SourceObligation {
            id: Uuid::from_u128(50).to_string(),
            source_input_id: Uuid::from_u128(50).to_string(),
            statement_digest: D.into(),
            conditions: vec![SourceClause {
                id: "condition.a".into(),
                digest: D.into(),
            }],
            exceptions: vec![SourceClause {
                id: "exception.a".into(),
                digest: D.into(),
            }],
        }],
        emitted: vec![alternative],
        rejected: vec![],
        baseline_id: id.clone(),
        ordered_ids: vec![id.clone()],
        eligible_set_digest: String::new(),
        whole_set_digest: String::new(),
    };
    manifest.eligible_set_digest = manifest.canonical_eligible_set_digest(&digest).unwrap();
    manifest.whole_set_digest = manifest.canonical_whole_set_digest(&digest).unwrap();
    manifest.validate(&digest).unwrap();
    AntiBloatInput {
        selected_revision: manifest.source.candidate_set_revision + 1,
        manifest,
        selected_id: id,
        graph_provenance: "trusted-fixture-binding".into(),
        dependency_digest: D.into(),
        obligation_links: vec![AntiBloatObligationLink {
            obligation_id: Uuid::from_u128(50).to_string(),
            goal_id: required_goal,
        }],
        non_goal_source_obligation_ids: vec![],
        mandatory_policy_obligation_ids: vec![Uuid::from_u128(50).to_string()],
    }
}

fn refresh(mut value: AntiBloatInput) -> AntiBloatInput {
    let digest = Sha256ScopeDigest;
    let alternative = &mut value.manifest.emitted[0];
    alternative.material_digest =
        scope_candidate_material_digest(&digest, &alternative.material).unwrap();
    alternative.id = stable_scope_alternative_id(
        &digest,
        &value.manifest.constructor,
        &value.manifest.source.digest,
        alternative.kind,
        &alternative.material_digest,
        &alternative.coverage,
    )
    .unwrap();
    value.selected_id = alternative.id.clone();
    value.manifest.baseline_id = alternative.id.clone();
    value.manifest.ordered_ids = vec![alternative.id.clone()];
    value.manifest.eligible_set_digest = value
        .manifest
        .canonical_eligible_set_digest(&digest)
        .unwrap();
    value.manifest.whole_set_digest = value.manifest.canonical_whole_set_digest(&digest).unwrap();
    value.manifest.validate(&digest).unwrap();
    value
}

#[derive(Default)]
struct FakeStore {
    selected_profile: Option<String>,
    mode: WorkspaceAdvisoryMode,
    input: Option<AntiBloatInput>,
    saved: Option<StoredAntiBloatReview>,
    sends: usize,
    seals: usize,
    applies: usize,
    prepared: Option<AntiBloatPreparedRequest>,
    raw_response: Option<Vec<u8>>,
    response_sha256: Option<String>,
    after: Option<ResolvedCandidateDraft>,
    applied: Option<AppliedDecision>,
    policy: Option<AdvisoryBudgetPolicy>,
    consumed: Option<AntiBloatProviderObservation>,
    sealed_observation: Option<AntiBloatProviderObservation>,
    fail_recovery_read: bool,
    fail_consume: bool,
    fail_finish: bool,
    consumption_count: usize,
}

fn approved_policy() -> AdvisoryBudgetPolicy {
    let id = Uuid::from_u128(500);
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 1,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 100_000,
        elapsed_monotonic_ms: 1000,
        retry_dispatches: 1,
    };
    AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, 0, i64::MAX, ceilings),
        0,
        i64::MAX,
        ceilings,
        Uuid::from_u128(501),
        "a".repeat(128),
    )
    .unwrap()
}

struct AppliedDecision {
    review_id: Uuid,
    input: AntiBloatInput,
    finding_id: String,
    disposition: AntiBloatDisposition,
    preservation: AntiBloatPreservation,
    delta: CandidateDeltaBatch,
    after: ResolvedCandidateDraft,
    receipt: AntiBloatApplyReceipt,
}

#[async_trait]
impl AntiBloatStore for FakeStore {
    async fn provider_profile_matches(&mut self, _: Uuid, profile: &str) -> Result<bool> {
        Ok(self.selected_profile.as_deref() == Some(profile))
    }
    async fn record_preflight_no_call(&mut self, _: Uuid, reason: AntiBloatNoCall) -> Result<()> {
        assert_eq!(
            self.saved.as_ref().unwrap().state,
            AntiBloatAttemptState::Prepared
        );
        assert!(self.prepared.is_none());
        assert_eq!(self.sends, 0);
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::NoCall(reason);
        Ok(())
    }
    async fn authorized_budget_policy(
        &mut self,
        _: Uuid,
        _: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>> {
        Ok(self.policy.clone())
    }
    async fn advisory_mode(&mut self, _: Uuid) -> Result<WorkspaceAdvisoryMode> {
        Ok(self.mode)
    }
    async fn authoritative_input(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: i64,
    ) -> Result<Option<AntiBloatInput>> {
        Ok(self.input.clone())
    }
    async fn save_review(
        &mut self,
        record: StoredAntiBloatReview,
    ) -> Result<StoredAntiBloatReview> {
        self.saved = Some(record.clone());
        Ok(record)
    }
    async fn review(&mut self, _: Uuid) -> Result<Option<StoredAntiBloatReview>> {
        Ok(self.saved.clone())
    }
    async fn begin_send(
        &mut self,
        saved: &StoredAntiBloatReview,
        prepared: &AntiBloatPreparedRequest,
        _: &AdvisoryBudgetPolicy,
        _: Option<&str>,
    ) -> Result<Option<AntiBloatSendPermit>> {
        if self.sends != 0 {
            return Ok(None);
        }
        self.sends += 1;
        self.prepared = Some(prepared.clone());
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::Sending;
        Ok(Some(AntiBloatSendPermit {
            review_id: saved.review_id,
            request: prepared.clone(),
        }))
    }
    async fn mark_send_unknown(&mut self, _: Uuid) -> Result<()> {
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::SendUnknown;
        Ok(())
    }
    async fn seal_ranked(&mut self, _: Uuid, ranked: &[String]) -> Result<()> {
        if self.fail_finish {
            self.fail_finish = false;
            return Err(Error::StorageUnavailable);
        }
        self.seals += 1;
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::Ranked(ranked.to_vec());
        Ok(())
    }
    async fn seal_response(
        &mut self,
        permit: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
        sha256: &str,
    ) -> Result<()> {
        assert_eq!(self.prepared.as_ref(), Some(&permit.request));
        assert_eq!(
            format!("{:x}", sha2::Sha256::digest(&observation.raw)),
            sha256
        );
        self.raw_response = Some(observation.raw.clone());
        self.sealed_observation = Some(observation.clone());
        self.response_sha256 = Some(sha256.into());
        Ok(())
    }
    async fn consume_budget(
        &mut self,
        _: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<bool> {
        if self.fail_consume {
            self.fail_consume = false;
            return Err(Error::StorageUnavailable);
        }
        assert_eq!(
            self.raw_response.as_deref(),
            Some(observation.raw.as_slice())
        );
        if let Some(saved) = &self.consumed {
            assert_eq!(saved, observation);
        } else {
            self.consumption_count += 1;
            self.consumed = Some(observation.clone());
        }
        Ok(observation.input_tokens.is_none()
            || observation.output_tokens.is_none()
            || observation.elapsed_monotonic_ms.is_none()
            || observation.input_tokens.is_some_and(|n| n > 100)
            || observation.output_tokens.is_some_and(|n| n > 100)
            || observation.elapsed_monotonic_ms.is_some_and(|n| n > 1000))
    }
    async fn authorized_sealed_response(
        &mut self,
        permit: &AntiBloatSendPermit,
    ) -> Result<AntiBloatProviderObservation> {
        if self.prepared.as_ref() != Some(&permit.request) {
            return Err(Error::InputConflict);
        }
        let consumed = self.consumed.as_ref().ok_or(Error::InputConflict)?;
        if consumed.input_tokens.is_none()
            || consumed.output_tokens.is_none()
            || consumed.elapsed_monotonic_ms.is_none()
            || consumed.input_tokens.is_some_and(|n| n > 100)
            || consumed.output_tokens.is_some_and(|n| n > 100)
            || consumed.elapsed_monotonic_ms.is_some_and(|n| n > 1000)
        {
            return Err(Error::InputConflict);
        }
        self.sealed_observation.clone().ok_or(Error::InputConflict)
    }
    async fn sealed_response_for_usage(
        &mut self,
        permit: &AntiBloatSendPermit,
    ) -> Result<AntiBloatProviderObservation> {
        if self.prepared.as_ref() != Some(&permit.request)
            || self.saved.as_ref().unwrap().state != AntiBloatAttemptState::Sending
        {
            return Err(Error::InputConflict);
        }
        self.sealed_observation.clone().ok_or(Error::InputConflict)
    }
    async fn saved_sealed_response(
        &mut self,
        review_id: Uuid,
    ) -> Result<Option<crate::AntiBloatSealedResponse>> {
        if self.fail_recovery_read {
            self.fail_recovery_read = false;
            return Err(Error::StorageUnavailable);
        }
        Ok(self
            .sealed_observation
            .clone()
            .map(|observation| crate::AntiBloatSealedResponse {
                permit: AntiBloatSendPermit {
                    review_id,
                    request: self.prepared.clone().unwrap(),
                },
                observation,
            }))
    }
    async fn seal_terminal(
        &mut self,
        permit: &AntiBloatSendPermit,
        state: AntiBloatAttemptState,
    ) -> Result<()> {
        self.sealed_response_for_usage(permit).await?;
        if self.consumed.is_none() {
            return Err(Error::InputConflict);
        }
        if state == AntiBloatAttemptState::ProviderAbstained {
            self.authorized_sealed_response(permit).await?;
        }
        assert!(matches!(
            state,
            AntiBloatAttemptState::ProviderAbstained | AntiBloatAttemptState::InvalidResponse
        ));
        self.saved.as_mut().unwrap().state = state;
        Ok(())
    }
    async fn apply_preserved_delta(
        &mut self,
        authored: &AntiBloatAuthoredDelta,
        input: &AntiBloatInput,
        preservation: &AntiBloatPreservation,
        after: &ResolvedCandidateDraft,
    ) -> Result<AntiBloatApplyReceipt> {
        let review_id = authored.review_id;
        let finding_id = authored.finding_id.as_str();
        let disposition = authored.disposition;
        let delta = &authored.delta;
        if let Some(applied) = &self.applied {
            if applied.review_id == review_id
                && &applied.input == input
                && applied.finding_id == finding_id
                && applied.disposition == disposition
                && &applied.preservation == preservation
                && &applied.delta == delta
                && &applied.after == after
            {
                return Ok(applied.receipt.clone());
            }
            return Err(Error::InputConflict);
        }
        if self.input.as_ref() != Some(input)
            || review_anti_bloat(&Sha256ScopeDigest, input)? != self.saved.as_ref().unwrap().review
        {
            return Err(Error::InputConflict);
        }
        self.applies += 1;
        self.after = Some(after.clone());
        let receipt = AntiBloatApplyReceipt {
            review_id,
            candidate_set_id: delta.candidate_set_id,
            idempotency_key: delta.idempotency_key.clone(),
            caller_request_id: Uuid::from_u128(999),
            from_revision: delta.expected_revision,
            to_revision: delta.expected_revision + 1,
            source_digest: preservation.source_digest.clone(),
            before_material_digest: preservation.before_material_digest.clone(),
            after_material_digest: preservation.after_material_digest.clone(),
        };
        self.applied = Some(AppliedDecision {
            review_id,
            input: input.clone(),
            finding_id: finding_id.into(),
            disposition,
            preservation: preservation.clone(),
            delta: delta.clone(),
            after: after.clone(),
            receipt: receipt.clone(),
        });
        Ok(receipt)
    }
}

struct FakeProvider {
    required_profile: Option<&'static str>,
    calls: AtomicUsize,
    invent: bool,
}

struct CommitObservingProvider {
    committed: Arc<AtomicBool>,
    calls: AtomicUsize,
    fail: bool,
}

#[async_trait]
impl AntiBloatRankingProvider for CommitObservingProvider {
    async fn rank(
        &self,
        started: &crate::AntiBloatStartedDispatchPermit,
    ) -> Result<AntiBloatProviderObservation> {
        started.claim()?;
        assert!(self.committed.load(Ordering::SeqCst));
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(Error::TransportUnavailable)
        } else {
            Ok(AntiBloatProviderObservation {
                response_complete: None,
                original_transport_context: None,
                http_status: None,
                raw: b"[]".to_vec(),
                input_tokens: Some(1),
                output_tokens: Some(1),
                elapsed_monotonic_ms: Some(1),
            })
        }
    }
}

#[tokio::test]
async fn provider_observes_committed_fence_and_commit_failure_never_calls() {
    let committed = Arc::new(AtomicBool::new(false));
    let provider = CommitObservingProvider {
        committed: committed.clone(),
        calls: AtomicUsize::new(0),
        fail: false,
    };
    let permit = AntiBloatSendPermit {
        review_id: Uuid::new_v4(),
        request: AntiBloatPreparedRequest {
            bytes: b"{}".to_vec(),
            sha256: format!("{:x}", Sha256::digest(b"{}")),
            material_sha256: "a".repeat(64),
            adapter_identity: "generic-json-v1".into(),
        },
    };
    let commit_flag = committed.clone();
    let observed = rank_after_committed_fence(
        async move {
            commit_flag.store(true, Ordering::SeqCst);
            Ok(())
        },
        &provider,
        &permit,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(observed.raw, b"[]");
    assert_eq!(observed.input_tokens, Some(1));
    assert_eq!(observed.output_tokens, Some(1));
    assert!(observed.elapsed_monotonic_ms.is_some_and(|n| n >= 0));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        rank_after_committed_fence(async { Err(Error::InputConflict) }, &provider, &permit).await,
        Err(Error::InputConflict)
    ));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let failing = CommitObservingProvider {
        committed: committed.clone(),
        calls: AtomicUsize::new(0),
        fail: true,
    };
    assert!(matches!(
        rank_after_committed_fence(async { Ok(()) }, &failing, &permit).await,
        Ok(Err(Error::TransportUnavailable))
    ));
    assert_eq!(failing.calls.load(Ordering::SeqCst), 1);
}

#[async_trait]
impl AntiBloatRankingProvider for FakeProvider {
    fn required_profile(&self) -> Option<&str> {
        self.required_profile
    }
    async fn rank(
        &self,
        started: &crate::AntiBloatStartedDispatchPermit,
    ) -> Result<AntiBloatProviderObservation> {
        let permit = started.claim()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.invent {
            Ok(AntiBloatProviderObservation {
                response_complete: None,
                original_transport_context: None,
                http_status: None,
                raw: br#"["invented"]"#.to_vec(),
                input_tokens: Some(1),
                output_tokens: Some(1),
                elapsed_monotonic_ms: Some(1),
            })
        } else {
            let request: serde_json::Value = serde_json::from_slice(&permit.request.bytes).unwrap();
            Ok(AntiBloatProviderObservation {
                response_complete: None,
                original_transport_context: None,
                http_status: None,
                raw: serde_json::to_vec(&request["eligible_ids"]).unwrap(),
                input_tokens: Some(1),
                output_tokens: Some(1),
                elapsed_monotonic_ms: Some(1),
            })
        }
    }
}

fn app(extra: bool, invent: bool) -> AntiBloatApplication<FakeStore, FakeProvider> {
    AntiBloatApplication {
        store: FakeStore {
            input: Some(input(extra)),
            policy: Some(approved_policy()),
            ..FakeStore::default()
        },
        provider: FakeProvider {
            required_profile: None,
            calls: AtomicUsize::new(0),
            invent,
        },
    }
}

async fn prepare(
    app: &mut AntiBloatApplication<FakeStore, FakeProvider>,
    mode: WorkspaceAdvisoryMode,
    preference: AdvisoryRequestPreference,
) -> StoredAntiBloatReview {
    app.store.mode = mode;
    app.prepare(
        Uuid::from_u128(10),
        Uuid::from_u128(11),
        Uuid::from_u128(1),
        4,
        preference,
    )
    .await
    .unwrap()
}

mod classification;
mod contract;
mod lifecycle;
mod preflight;
mod provider_seams;
mod recovery;
mod scenarios;
mod started_dispatch;
