use super::write::validate_review;
use tect_domain::{
    CandidateBoundary, CandidateReviewDraft, CoverageGoalEntity, CoverageResolutionEntity,
    CoverageResolutionKind, EmptyCandidateDisposition, EmptyCandidateDispositionKind,
    EvidenceEntity, EvidenceKind, ResolvedCandidateDraft, ReviewCandidateSet, ReviewVerdict,
};
use uuid::Uuid;

fn request(verdict: ReviewVerdict) -> ReviewCandidateSet {
    ReviewCandidateSet {
        candidate_set_id: Uuid::new_v4(),
        revision: 2,
        snapshot_id: Uuid::new_v4(),
        input_cursor: 1,
        request_id: Uuid::new_v4(),
        review: CandidateReviewDraft {
            verdict,
            summary: "Substantive review of the captured disposition".into(),
            findings: Vec::new(),
            candidate_decisions: Vec::new(),
            protected_change_reviews: Vec::new(),
        },
    }
}

fn empty(
    kind: Option<EmptyCandidateDispositionKind>,
    pending: Option<&str>,
) -> ResolvedCandidateDraft {
    ResolvedCandidateDraft {
        boundary: CandidateBoundary::Ongoing,
        goals: Vec::new(),
        evidence: Vec::new(),
        candidates: Vec::new(),
        blockers: Vec::new(),
        pending_question: pending.map(str::to_owned),
        empty_disposition: kind.map(|kind| EmptyCandidateDisposition {
            kind,
            reason: "Captured source explains the empty candidate set".into(),
            source_ref_id: Uuid::new_v4(),
        }),
        protected_changes: Vec::new(),
        delta: Default::default(),
    }
}

#[test]
fn empty_set_reviews_are_blocked_honestly_and_only_real_coverage_is_ready() {
    for draft in [
        empty(
            Some(EmptyCandidateDispositionKind::NeedsInput),
            Some("Which authority applies?"),
        ),
        empty(Some(EmptyCandidateDispositionKind::OutOfBoundary), None),
    ] {
        assert!(validate_review(&request(ReviewVerdict::Blocked), &draft).is_ok());
        assert!(validate_review(&request(ReviewVerdict::Ready), &draft).is_err());
    }
    assert!(validate_review(&request(ReviewVerdict::Ready), &empty(None, None)).is_err());

    let evidence_id = Uuid::new_v4();
    let all_covered = ResolvedCandidateDraft {
        boundary: CandidateBoundary::Finite,
        goals: vec![CoverageGoalEntity {
            id: Uuid::new_v4(),
            revision: 1,
            text: "Finite success is already delivered".into(),
            source_ref_id: Uuid::new_v4(),
            exact_quote: None,
            resolution: CoverageResolutionEntity {
                kind: CoverageResolutionKind::Evidence,
                id: evidence_id,
            },
        }],
        evidence: vec![EvidenceEntity {
            id: evidence_id,
            revision: 1,
            kind: EvidenceKind::VerifiedEvidence,
            summary: "Verified delivered result".into(),
            source_ref_id: Uuid::new_v4(),
            authority_input_sequence: None,
        }],
        candidates: Vec::new(),
        blockers: Vec::new(),
        pending_question: None,
        empty_disposition: Some(EmptyCandidateDisposition {
            kind: EmptyCandidateDispositionKind::AllCovered,
            reason: "Every finite outcome maps to verified evidence".into(),
            source_ref_id: Uuid::new_v4(),
        }),
        protected_changes: Vec::new(),
        delta: Default::default(),
    };
    assert!(validate_review(&request(ReviewVerdict::Ready), &all_covered).is_ok());
}
