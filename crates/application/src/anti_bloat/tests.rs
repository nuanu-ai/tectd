use super::*;
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
            id: "program.intent".into(),
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
        obligation_id: "obligation.intent".into(),
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
            id: "obligation.intent".into(),
            source_input_id: "program.intent".into(),
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
        manifest,
        selected_id: id,
        graph_provenance: "trusted-fixture-binding".into(),
        dependency_digest: D.into(),
        obligation_links: vec![AntiBloatObligationLink {
            obligation_id: "obligation.intent".into(),
            goal_id: required_goal,
        }],
        mandatory_policy_obligation_ids: vec!["obligation.intent".into()],
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
}

struct AppliedDecision {
    review_id: Uuid,
    input: AntiBloatInput,
    finding_id: String,
    disposition: AntiBloatDisposition,
    preservation: AntiBloatPreservation,
    delta: CandidateDeltaBatch,
    after: ResolvedCandidateDraft,
    receipt: CandidateDeltaReceipt,
}

#[async_trait]
impl AntiBloatStore for FakeStore {
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
        self.seals += 1;
        self.saved.as_mut().unwrap().state = AntiBloatAttemptState::Ranked(ranked.to_vec());
        Ok(())
    }
    async fn seal_response(
        &mut self,
        permit: &AntiBloatSendPermit,
        raw: &[u8],
        sha256: &str,
    ) -> Result<()> {
        assert_eq!(self.prepared.as_ref(), Some(&permit.request));
        assert_eq!(format!("{:x}", sha2::Sha256::digest(raw)), sha256);
        self.raw_response = Some(raw.to_vec());
        self.response_sha256 = Some(sha256.into());
        Ok(())
    }
    async fn apply_preserved_delta(
        &mut self,
        review_id: Uuid,
        input: &AntiBloatInput,
        finding_id: &str,
        disposition: AntiBloatDisposition,
        preservation: &AntiBloatPreservation,
        delta: &CandidateDeltaBatch,
        after: &ResolvedCandidateDraft,
    ) -> Result<CandidateDeltaReceipt> {
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
        let receipt = CandidateDeltaReceipt {
            candidate_set_id: delta.candidate_set_id,
            idempotency_key: delta.idempotency_key.clone(),
            from_revision: delta.expected_revision,
            to_revision: delta.expected_revision + 1,
            stale_reasons: vec![],
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
    async fn rank(&self, _: &AntiBloatSendPermit) -> Result<Vec<u8>> {
        assert!(self.committed.load(Ordering::SeqCst));
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail {
            Err(Error::TransportUnavailable)
        } else {
            Ok(b"[]".to_vec())
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
            sha256: "a".repeat(64),
        },
    };
    let commit_flag = committed.clone();
    assert_eq!(
        rank_after_committed_fence(
            async move {
                commit_flag.store(true, Ordering::SeqCst);
                Ok(())
            },
            &provider,
            &permit
        )
        .await
        .unwrap()
        .unwrap(),
        b"[]".to_vec()
    );
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
    async fn rank(&self, permit: &AntiBloatSendPermit) -> Result<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.invent {
            Ok(br#"["invented"]"#.to_vec())
        } else {
            let request: serde_json::Value = serde_json::from_slice(&permit.request.bytes).unwrap();
            Ok(serde_json::to_vec(&request["eligible_ids"]).unwrap())
        }
    }
}

fn app(extra: bool, invent: bool) -> AntiBloatApplication<FakeStore, FakeProvider> {
    AntiBloatApplication {
        store: FakeStore {
            input: Some(input(extra)),
            ..FakeStore::default()
        },
        provider: FakeProvider {
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
        3,
        preference,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn no_call_states_are_durable_and_never_send() {
    for (extra, mode, preference, expected) in [
        (
            true,
            WorkspaceAdvisoryMode::Disabled,
            AdvisoryRequestPreference::UseWorkspace,
            AntiBloatNoCall::Disabled,
        ),
        (
            true,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::Skip,
            AntiBloatNoCall::Skipped,
        ),
        (
            false,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::UseWorkspace,
            AntiBloatNoCall::NoEligibleFindings,
        ),
    ] {
        let mut app = app(extra, false);
        let saved = prepare(&mut app, mode, preference).await;
        assert_eq!(saved.state, AntiBloatAttemptState::NoCall(expected));
        let attempt = app.prepare_send(saved.review_id).await.unwrap();
        assert_eq!(attempt.state, saved.state);
        assert!(attempt.permit.is_none());
        assert_eq!(app.store.sends, 0);
        assert_eq!(app.provider.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn one_use_rank_and_provider_invention_is_denied() {
    let mut app = app(true, false);
    let saved = prepare(
        &mut app,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    let attempt = app.prepare_send(saved.review_id).await.unwrap();
    assert_eq!(attempt.state, AntiBloatAttemptState::Sending);
    assert_eq!(app.provider.calls.load(Ordering::SeqCst), 0);
    assert!(app.store.raw_response.is_none());
    let permit = attempt.permit.unwrap();
    let raw = app.provider.rank(&permit).await.unwrap();
    app.seal_response(&permit, &raw).await.unwrap();
    assert_eq!(app.store.seals, 0);
    assert_eq!(
        app.finalize_response(&permit, &raw).await.unwrap(),
        app.store.saved.as_ref().unwrap().state
    );
    assert!(
        app.prepare_send(saved.review_id)
            .await
            .unwrap()
            .permit
            .is_none()
    );
    assert_eq!(app.store.sends, 1);
    assert_eq!(app.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(app.store.applies, 0);
    let prepared = app.store.prepared.as_ref().unwrap();
    assert_eq!(
        prepared.sha256,
        format!("{:x}", sha2::Sha256::digest(&prepared.bytes))
    );
    let request: serde_json::Value = serde_json::from_slice(&prepared.bytes).unwrap();
    assert_eq!(
        request["review"],
        serde_json::to_value(&saved.review).unwrap()
    );
    let raw = app.store.raw_response.as_ref().unwrap();
    assert_eq!(
        app.store.response_sha256.as_ref().unwrap(),
        &format!("{:x}", sha2::Sha256::digest(raw))
    );
    let mut invented = self::app(true, true);
    let saved = prepare(
        &mut invented,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    let permit = invented
        .prepare_send(saved.review_id)
        .await
        .unwrap()
        .permit
        .unwrap();
    let raw = invented.provider.rank(&permit).await.unwrap();
    invented.seal_response(&permit, &raw).await.unwrap();
    assert!(matches!(
        invented.finalize_response(&permit, &raw).await,
        Err(Error::InputConflict)
    ));
    assert_eq!(invented.store.seals, 0);
    assert_eq!(
        invented.store.raw_response.as_deref(),
        Some(br#"["invented"]"#.as_slice())
    );
    assert_eq!(
        invented.store.saved.as_ref().unwrap().state,
        AntiBloatAttemptState::SendUnknown
    );
    assert_eq!(
        invented.prepare_send(saved.review_id).await.unwrap().state,
        AntiBloatAttemptState::SendUnknown
    );
    assert_eq!(invented.provider.calls.load(Ordering::SeqCst), 1);
}

mod classification;
mod contract;
