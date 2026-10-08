use super::*;
use tect_domain::{
    BlockerEntity, CandidateDecision, CandidateDecisionKind, CandidateEntity, CandidateFinding,
    CandidateFindingSeverity, CandidateReviewDraft, ProtectedChangeEntity, ProtectedChangeReview,
    ResolvedCandidateDraft, ReviewCandidateSet, ReviewVerdict,
};

fn candidate(id: Uuid) -> CandidateEntity {
    CandidateEntity {
        id,
        revision: 1,
        title: "Candidate title".into(),
        outcome: "Candidate outcome".into(),
        trigger: "Candidate trigger".into(),
        delivered_behavior: "Candidate behavior".into(),
        proof: "Candidate proof".into(),
        includes: Vec::new(),
        excludes: Vec::new(),
        dependencies: Vec::new(),
        coverage_goal_ids: Vec::new(),
        evidence_ids: Vec::new(),
    }
}

fn draft(candidate_ids: &[Uuid]) -> ResolvedCandidateDraft {
    ResolvedCandidateDraft {
        boundary: tect_domain::CandidateBoundary::Ongoing,
        goals: Vec::new(),
        evidence: Vec::new(),
        candidates: candidate_ids.iter().copied().map(candidate).collect(),
        blockers: Vec::new(),
        pending_question: None,
        empty_disposition: None,
        protected_changes: Vec::new(),
        delta: Default::default(),
    }
}

fn decision(
    candidate_id: Uuid,
    decision: CandidateDecisionKind,
    rationale: &str,
) -> CandidateDecision {
    CandidateDecision {
        candidate_id,
        decision,
        rationale: rationale.into(),
    }
}

fn request(candidate_ids: &[Uuid]) -> ReviewCandidateSet {
    ReviewCandidateSet {
        candidate_set_id: Uuid::new_v4(),
        revision: 1,
        snapshot_id: Uuid::new_v4(),
        input_cursor: 0,
        request_id: Uuid::new_v4(),
        review: CandidateReviewDraft {
            verdict: ReviewVerdict::Ready,
            summary: "Reviewed the current candidates".into(),
            findings: Vec::new(),
            candidate_decisions: candidate_ids
                .iter()
                .copied()
                .map(|id| decision(id, CandidateDecisionKind::Accept, "In scope"))
                .collect(),
            protected_change_reviews: Vec::new(),
        },
        consumed_knowledge: None,
    }
}

fn assert_diagnostic(result: Result<()>, pointer: &str, reason: &str) {
    let error = result.expect_err("review should fail validation");
    let diagnostic = error
        .argument_diagnostic()
        .expect("semantic review errors should retain an argument diagnostic");
    assert_eq!(diagnostic.pointer, pointer);
    assert!(diagnostic.reason.contains(reason), "{}", diagnostic.reason);
}

#[test]
fn review_with_one_decision_for_every_current_candidate_is_valid() {
    let ids = [Uuid::new_v4(), Uuid::new_v4()];
    let result = validate_review(&request(&ids), &draft(&ids));
    assert!(result.is_ok());
}

#[test]
fn added_candidate_omission_reports_missing_id_and_path() {
    let existing = [Uuid::new_v4(), Uuid::new_v4()];
    let added = Uuid::new_v4();
    let mut current_ids = existing.to_vec();
    current_ids.push(added);

    let result = validate_review(&request(&existing), &draft(&current_ids));

    assert_diagnostic(
        result,
        "/params/review/candidate_decisions",
        &format!("missing candidate IDs [{added}]"),
    );
}

#[test]
fn extra_candidate_decision_reports_unknown_id_and_path() {
    let current = Uuid::new_v4();
    let extra = Uuid::new_v4();
    let mut review = request(&[current]);
    review.review.candidate_decisions.push(decision(
        extra,
        CandidateDecisionKind::Accept,
        "In scope",
    ));

    assert_diagnostic(
        validate_review(&review, &draft(&[current])),
        "/params/review/candidate_decisions",
        &format!("unknown candidate IDs [{extra}]"),
    );
}

#[test]
fn duplicate_candidate_decisions_report_duplicate_id_and_path() {
    let current = Uuid::new_v4();
    let mut review = request(&[current]);
    review.review.candidate_decisions.push(decision(
        current,
        CandidateDecisionKind::Accept,
        "Again",
    ));

    assert_diagnostic(
        validate_review(&review, &draft(&[current])),
        "/params/review/candidate_decisions",
        &format!("duplicate candidate IDs [{current}]"),
    );
}

#[test]
fn long_membership_errors_bound_ids_and_report_omitted_count() {
    let ids: Vec<_> = (1..=10).map(Uuid::from_u128).collect();
    let error = validate_review(&request(&[]), &draft(&ids)).unwrap_err();
    let diagnostic = error.argument_diagnostic().unwrap();

    assert_eq!(diagnostic.pointer, "/params/review/candidate_decisions");
    assert!(diagnostic.reason.contains("and 7 more"));
    assert!(diagnostic.reason.len() < 600);
}

#[test]
fn blank_summary_and_candidate_rationale_report_nested_paths() {
    let id = Uuid::new_v4();
    let mut review = request(&[id]);
    review.review.summary = " \n ".into();
    assert_diagnostic(
        validate_review(&review, &draft(&[id])),
        "/params/review/summary",
        "review summary must be nonblank",
    );

    review.review.summary = "A substantive review".into();
    review.review.candidate_decisions[0].rationale = "  ".into();
    assert_diagnostic(
        validate_review(&review, &draft(&[id])),
        "/params/review/candidate_decisions/0/rationale",
        "requires a nonblank rationale",
    );
}

#[test]
fn protected_change_coverage_and_rationale_report_paths() {
    let id = Uuid::new_v4();
    let evidence_id = Uuid::new_v4();
    let prior_candidate_id = Uuid::new_v4();
    let mut current = draft(&[id]);
    current.protected_changes.push(ProtectedChangeEntity {
        accepted_evidence_id: evidence_id,
        prior_candidate_id: Some(prior_candidate_id),
        disposition: tect_domain::ProtectedChangeDisposition::Delete,
        rationale: "Superseded by explicit authority".into(),
        authority_source_ref_id: Uuid::new_v4(),
        replacement_evidence_id: None,
        target_candidate_id: None,
    });
    let mut review = request(&[id]);

    assert_diagnostic(
        validate_review(&review, &current),
        "/params/review/protected_change_reviews",
        &format!("accepted_evidence_id={evidence_id}"),
    );

    review
        .review
        .protected_change_reviews
        .push(ProtectedChangeReview {
            accepted_evidence_id: evidence_id,
            prior_candidate_id: Some(prior_candidate_id),
            rationale: "  ".into(),
        });
    assert_diagnostic(
        validate_review(&review, &current),
        "/params/review/protected_change_reviews/0/rationale",
        "requires a nonblank rationale",
    );
}

#[test]
fn protected_change_membership_diagnostics_stay_bounded() {
    let id = Uuid::new_v4();
    let mut current = draft(&[id]);
    for value in 1..=10 {
        current.protected_changes.push(ProtectedChangeEntity {
            accepted_evidence_id: Uuid::from_u128(value),
            prior_candidate_id: Some(Uuid::from_u128(value + 100)),
            disposition: tect_domain::ProtectedChangeDisposition::Delete,
            rationale: "Superseded by explicit authority".into(),
            authority_source_ref_id: Uuid::new_v4(),
            replacement_evidence_id: None,
            target_candidate_id: None,
        });
    }
    let mut review = request(&[id]);
    for value in 20..=29 {
        review
            .review
            .protected_change_reviews
            .push(ProtectedChangeReview {
                accepted_evidence_id: Uuid::from_u128(value),
                prior_candidate_id: Some(Uuid::from_u128(value + 100)),
                rationale: "Reviewed".into(),
            });
    }
    review
        .review
        .protected_change_reviews
        .push(review.review.protected_change_reviews[0].clone());

    let error = validate_review(&review, &current).unwrap_err();
    let diagnostic = error.argument_diagnostic().unwrap();
    assert_eq!(
        diagnostic.pointer,
        "/params/review/protected_change_reviews"
    );
    assert!(
        diagnostic
            .reason
            .contains("missing protected-change reviews")
    );
    assert!(
        diagnostic
            .reason
            .contains("unknown protected-change reviews")
    );
    assert!(
        diagnostic
            .reason
            .contains("duplicate protected-change reviews")
    );
    assert!(diagnostic.reason.contains("and 9 more"));
    assert!(diagnostic.reason.len() < 600);
}

#[test]
fn findings_report_blank_text_and_unknown_references() {
    let id = Uuid::new_v4();
    let unknown = Uuid::new_v4();
    let mut review = request(&[id]);
    review.review.findings.push(CandidateFinding {
        severity: CandidateFindingSeverity::Advisory,
        summary: "  ".into(),
        candidate_ids: vec![unknown],
        coverage_goal_ids: Vec::new(),
        disposition: "Revise candidate wording".into(),
    });
    assert_diagnostic(
        validate_review(&review, &draft(&[id])),
        "/params/review/findings/0/summary",
        "requires a nonblank summary",
    );

    review.review.findings[0].summary = "Missing candidate evidence".into();
    assert_diagnostic(
        validate_review(&review, &draft(&[id])),
        "/params/review/findings/0/candidate_ids",
        &format!("unknown candidate IDs [{unknown}]"),
    );
}

#[test]
fn incompatible_ready_or_unsubstantiated_blocked_verdict_reports_verdict_path() {
    let id = Uuid::new_v4();
    let mut blocked_draft = draft(&[id]);
    blocked_draft.blockers.push(BlockerEntity {
        id: Uuid::new_v4(),
        revision: 1,
        summary: "A prerequisite remains unresolved".into(),
        source_ref_id: Uuid::new_v4(),
    });
    assert_diagnostic(
        validate_review(&request(&[id]), &blocked_draft),
        "/params/review/verdict",
        "ready verdict requires a draft with no blockers",
    );

    let mut unsupported = request(&[id]);
    unsupported.review.verdict = ReviewVerdict::Blocked;
    assert_diagnostic(
        validate_review(&unsupported, &draft(&[id])),
        "/params/review/verdict",
        "blocked verdict requires a blocker",
    );
}

#[test]
fn ready_verdict_reports_nonaccept_decision_path() {
    let id = Uuid::new_v4();
    let mut review = request(&[id]);
    review.review.candidate_decisions[0].decision = CandidateDecisionKind::Revise;

    assert_diagnostic(
        validate_review(&review, &draft(&[id])),
        "/params/review/candidate_decisions/0/decision",
        "every candidate decision to be accept",
    );
}
