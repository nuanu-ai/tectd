use std::collections::{BTreeMap, BTreeSet};
use tect_domain::{
    CandidateDecisionKind, CandidateFindingSeverity, Error, Result, ReviewCandidateSet,
    ReviewVerdict,
};
use uuid::Uuid;

pub(crate) fn validate_review(
    request: &ReviewCandidateSet,
    draft: &tect_domain::ResolvedCandidateDraft,
) -> Result<()> {
    if request.review.summary.trim().is_empty() || request.review.summary.contains('\0') {
        return Err(invalid_review(
            "review summary must be nonblank and contain no NUL",
            "/params/review/summary",
        ));
    }
    let candidate_ids: BTreeSet<_> = draft.candidates.iter().map(|v| v.id).collect();
    let mut decision_counts = BTreeMap::new();
    for decision in &request.review.candidate_decisions {
        *decision_counts
            .entry(decision.candidate_id)
            .or_insert(0usize) += 1;
    }
    let decision_ids: BTreeSet<_> = decision_counts.keys().copied().collect();
    let missing: Vec<_> = candidate_ids.difference(&decision_ids).copied().collect();
    let extra: Vec<_> = decision_ids.difference(&candidate_ids).copied().collect();
    let duplicate: Vec<_> = decision_counts
        .iter()
        .filter_map(|(id, count)| (*count > 1).then_some(*id))
        .collect();
    if !missing.is_empty() || !extra.is_empty() || !duplicate.is_empty() {
        let mut issues = Vec::new();
        if !missing.is_empty() {
            issues.push(format!(
                "missing candidate IDs {}",
                bounded_uuid_list(&missing)
            ));
        }
        if !extra.is_empty() {
            issues.push(format!(
                "unknown candidate IDs {}",
                bounded_uuid_list(&extra)
            ));
        }
        if !duplicate.is_empty() {
            issues.push(format!(
                "duplicate candidate IDs {}",
                bounded_uuid_list(&duplicate)
            ));
        }
        return Err(invalid_review(
            format!(
                "candidate_decisions must contain exactly one decision per current candidate; {}",
                issues.join("; ")
            ),
            "/params/review/candidate_decisions",
        ));
    }
    for (index, decision) in request.review.candidate_decisions.iter().enumerate() {
        if decision.rationale.trim().is_empty() {
            return Err(invalid_review(
                format!("candidate decision at index {index} requires a nonblank rationale"),
                format!("/params/review/candidate_decisions/{index}/rationale"),
            ));
        }
    }
    let protected: BTreeSet<_> = draft
        .protected_changes
        .iter()
        .map(|value| (value.accepted_evidence_id, value.prior_candidate_id))
        .collect();
    let mut protected_review_counts = BTreeMap::new();
    for review in &request.review.protected_change_reviews {
        *protected_review_counts
            .entry((review.accepted_evidence_id, review.prior_candidate_id))
            .or_insert(0usize) += 1;
    }
    let reviewed: BTreeSet<_> = protected_review_counts.keys().copied().collect();
    let missing: Vec<_> = protected.difference(&reviewed).copied().collect();
    let extra: Vec<_> = reviewed.difference(&protected).copied().collect();
    let duplicate: Vec<_> = protected_review_counts
        .iter()
        .filter_map(|(change, count)| (*count > 1).then_some(*change))
        .collect();
    if !missing.is_empty() || !extra.is_empty() || !duplicate.is_empty() {
        let mut issues = Vec::new();
        if !missing.is_empty() {
            issues.push(format!(
                "missing protected-change reviews {}",
                bounded_protected_change_list(&missing)
            ));
        }
        if !extra.is_empty() {
            issues.push(format!(
                "unknown protected-change reviews {}",
                bounded_protected_change_list(&extra)
            ));
        }
        if !duplicate.is_empty() {
            issues.push(format!(
                "duplicate protected-change reviews {}",
                bounded_protected_change_list(&duplicate)
            ));
        }
        return Err(invalid_review(
            format!(
                "protected_change_reviews must contain exactly one review per current protected change; {}",
                issues.join("; ")
            ),
            "/params/review/protected_change_reviews",
        ));
    }
    for (index, review) in request.review.protected_change_reviews.iter().enumerate() {
        if review.rationale.trim().is_empty() {
            return Err(invalid_review(
                format!("protected-change review at index {index} requires a nonblank rationale"),
                format!("/params/review/protected_change_reviews/{index}/rationale"),
            ));
        }
    }
    let goal_ids: BTreeSet<_> = draft.goals.iter().map(|v| v.id).collect();
    for (index, finding) in request.review.findings.iter().enumerate() {
        if finding.summary.trim().is_empty() {
            return Err(invalid_review(
                format!("finding at index {index} requires a nonblank summary"),
                format!("/params/review/findings/{index}/summary"),
            ));
        }
        if finding.disposition.trim().is_empty() {
            return Err(invalid_review(
                format!("finding at index {index} requires a nonblank disposition"),
                format!("/params/review/findings/{index}/disposition"),
            ));
        }
        let unknown_candidates: Vec<_> = finding
            .candidate_ids
            .iter()
            .filter(|id| !candidate_ids.contains(id))
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if !unknown_candidates.is_empty() {
            return Err(invalid_review(
                format!(
                    "finding at index {index} references unknown candidate IDs {}",
                    bounded_uuid_list(&unknown_candidates)
                ),
                format!("/params/review/findings/{index}/candidate_ids"),
            ));
        }
        let unknown_goals: Vec<_> = finding
            .coverage_goal_ids
            .iter()
            .filter(|id| !goal_ids.contains(id))
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        if !unknown_goals.is_empty() {
            return Err(invalid_review(
                format!(
                    "finding at index {index} references unknown coverage-goal IDs {}",
                    bounded_uuid_list(&unknown_goals)
                ),
                format!("/params/review/findings/{index}/coverage_goal_ids"),
            ));
        }
    }
    let material = request
        .review
        .findings
        .iter()
        .any(|v| v.severity == CandidateFindingSeverity::Material);
    let empty_ready = draft.candidates.is_empty()
        && draft.empty_disposition.as_ref().is_some_and(|value| {
            value.kind == tect_domain::EmptyCandidateDispositionKind::AllCovered
        });
    let empty_blocked = draft.candidates.is_empty()
        && draft.empty_disposition.as_ref().is_some_and(|value| {
            matches!(
                value.kind,
                tect_domain::EmptyCandidateDispositionKind::NeedsInput
                    | tect_domain::EmptyCandidateDispositionKind::OutOfBoundary
            )
        });
    match request.review.verdict {
        ReviewVerdict::Ready => {
            if !draft.blockers.is_empty() {
                return Err(invalid_review(
                    "ready verdict requires a draft with no blockers",
                    "/params/review/verdict",
                ));
            }
            if material {
                return Err(invalid_review(
                    "ready verdict is not allowed while a material finding remains",
                    "/params/review/verdict",
                ));
            }
            if draft.pending_question.is_some() {
                return Err(invalid_review(
                    "ready verdict is not allowed while a pending question remains",
                    "/params/review/verdict",
                ));
            }
            if draft.candidates.is_empty() && !empty_ready {
                return Err(invalid_review(
                    "ready verdict for an empty candidate set requires all_covered disposition",
                    "/params/review/verdict",
                ));
            }
            if let Some((index, _)) = request
                .review
                .candidate_decisions
                .iter()
                .enumerate()
                .find(|(_, decision)| decision.decision != CandidateDecisionKind::Accept)
            {
                return Err(invalid_review(
                    "ready verdict requires every candidate decision to be accept",
                    format!("/params/review/candidate_decisions/{index}/decision"),
                ));
            }
            Ok(())
        }
        ReviewVerdict::Blocked if draft.blockers.is_empty() && !material && !empty_blocked => {
            Err(invalid_review(
                "blocked verdict requires a blocker, material finding, or empty needs_input/out_of_boundary disposition",
                "/params/review/verdict",
            ))
        }
        _ => Ok(()),
    }
}

const MAX_REVIEW_DIAGNOSTIC_IDS: usize = 3;

fn invalid_review(reason: impl std::fmt::Display, pointer: impl Into<String>) -> Error {
    Error::invalid_arguments_at(reason, pointer)
}

fn bounded_uuid_list(ids: &[Uuid]) -> String {
    let mut listed: Vec<_> = ids
        .iter()
        .take(MAX_REVIEW_DIAGNOSTIC_IDS)
        .map(ToString::to_string)
        .collect();
    if ids.len() > MAX_REVIEW_DIAGNOSTIC_IDS {
        listed.push(format!(
            "and {} more",
            ids.len() - MAX_REVIEW_DIAGNOSTIC_IDS
        ));
    }
    format!("[{}]", listed.join(", "))
}

fn bounded_protected_change_list(changes: &[(Uuid, Option<Uuid>)]) -> String {
    let mut listed: Vec<_> = changes
        .iter()
        .take(1)
        .map(|(evidence_id, prior_candidate_id)| {
            format!(
                "(accepted_evidence_id={evidence_id}, prior_candidate_id={})",
                prior_candidate_id.map_or_else(|| "none".to_owned(), |id| id.to_string())
            )
        })
        .collect();
    if changes.len() > 1 {
        listed.push(format!("and {} more", changes.len() - 1));
    }
    format!("[{}]", listed.join(", "))
}
