use crate::*;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const D: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub(super) struct TestScopeDigest;

impl ScopeDigest for TestScopeDigest {
    fn sha256(&self, domain: &'static str, canonical_bytes: &[u8]) -> String {
        let mut digest = Sha256::new();
        digest.update(domain.as_bytes());
        digest.update([0]);
        digest.update(canonical_bytes);
        format!("{:x}", digest.finalize())
    }
}

pub(super) fn digest() -> TestScopeDigest {
    TestScopeDigest
}

pub(super) fn fixture_manifest() -> ScopeConstructorManifest {
    let source_ref = Uuid::from_u128(50);
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
    source.digest = source.canonical_digest(&digest()).unwrap();
    let obligations = vec![SourceObligation {
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
    }];
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
    let mut emitted = [
        (ScopeDecompositionKind::Cohesive, "cohesive"),
        (ScopeDecompositionKind::Partitioned, "partitioned"),
    ]
    .into_iter()
    .map(|(kind, title)| {
        let material = draft(source_ref, title);
        let material_digest = scope_candidate_material_digest(&digest(), &material).unwrap();
        let id = stable_scope_alternative_id(
            &digest(),
            &constructor,
            &source.digest,
            kind,
            &material_digest,
            &coverage,
        )
        .unwrap();
        ScopeDecompositionAlternative {
            id,
            kind,
            material,
            material_digest,
            coverage: coverage.clone(),
        }
    })
    .collect::<Vec<_>>();
    emitted.sort_by(|left, right| left.id.cmp(&right.id));
    let mut ordered_ids = emitted
        .iter()
        .map(|value| value.id.clone())
        .collect::<Vec<_>>();
    ordered_ids.sort();
    let baseline_id = emitted[0].id.clone();
    let mut manifest = ScopeConstructorManifest {
        constructor,
        source,
        obligations,
        emitted,
        rejected: Vec::new(),
        baseline_id,
        ordered_ids,
        eligible_set_digest: String::new(),
        whole_set_digest: String::new(),
    };
    manifest.eligible_set_digest = manifest.canonical_eligible_set_digest(&digest()).unwrap();
    manifest.whole_set_digest = manifest.canonical_whole_set_digest(&digest()).unwrap();
    manifest
}

fn draft(source_ref: Uuid, title: &str) -> ResolvedCandidateDraft {
    let goal_id = Uuid::from_u128(51);
    let candidate_id = Uuid::from_u128(52);
    ResolvedCandidateDraft {
        boundary: CandidateBoundary::Finite,
        goals: vec![CoverageGoalEntity {
            id: goal_id,
            revision: 1,
            text: "Preserve the source outcome".into(),
            source_ref_id: source_ref,
            exact_quote: None,
            resolution: CoverageResolutionEntity {
                kind: CoverageResolutionKind::Candidate,
                id: candidate_id,
            },
        }],
        evidence: Vec::new(),
        candidates: vec![CandidateEntity {
            id: candidate_id,
            revision: 1,
            title: title.into(),
            outcome: "Exact supplied outcome".into(),
            trigger: "Exact supplied trigger".into(),
            delivered_behavior: "Exact supplied behavior".into(),
            proof: "Exact supplied proof".into(),
            includes: vec!["supplied".into()],
            excludes: Vec::new(),
            dependencies: Vec::new(),
            coverage_goal_ids: vec![goal_id],
            evidence_ids: Vec::new(),
        }],
        blockers: Vec::new(),
        pending_question: None,
        empty_disposition: None,
        protected_changes: Vec::new(),
        delta: CandidateDelta {
            added: vec![CandidateAdded {
                candidate_id,
                revision: 1,
            }],
            ..CandidateDelta::default()
        },
    }
}

pub(super) fn answers(
    manifest: &ScopeConstructorManifest,
    scores: [ScopeAdviceScoreBand; 2],
) -> NormalizedScopeAdviceAnswers {
    NormalizedScopeAdviceAnswers {
        answers: manifest
            .emitted
            .iter()
            .zip(scores)
            .map(|(alternative, score)| NormalizedScopeAdviceAnswer {
                alternative_id: alternative.id.clone(),
                choice: ScopeAdviceChoice::Preferred,
                score,
                choice_confidence: ConfidenceBasisPoints(8_000),
                score_confidence: ConfidenceBasisPoints(7_000),
            })
            .collect(),
    }
}

pub(super) fn guarded(manifest: &ScopeConstructorManifest) -> GuardedScopeAdvice {
    guard_scope_advice(
        &digest(),
        manifest,
        &ScopeAdviceRequest::from_manifest(&digest(), manifest).unwrap(),
        &answers(
            manifest,
            [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::WeakFit],
        ),
    )
    .unwrap()
}
