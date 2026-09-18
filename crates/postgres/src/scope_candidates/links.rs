//! Goal/candidate coverage links and dependency order of a resolved draft.

use std::collections::{BTreeMap, BTreeSet};
use tect_domain::{CandidateEntity, CoverageGoalEntity, CoverageResolutionKind, Error, Result};
use uuid::Uuid;

/// Draft-local labels by allocated entity id, so reasons name what the agent wrote.
pub(super) type Labels = BTreeMap<Uuid, String>;

fn name(labels: &Labels, id: Uuid) -> String {
    labels.get(&id).cloned().unwrap_or_else(|| id.to_string())
}

/// A candidate-resolved goal and the candidate's coverage list name each other;
/// a goal is covered by exactly one candidate.
pub(super) fn validate_candidate_links(
    goals: &[CoverageGoalEntity],
    candidates: &[CandidateEntity],
    labels: &Labels,
) -> Result<()> {
    for goal in goals {
        if goal.resolution.kind != CoverageResolutionKind::Candidate {
            continue;
        }
        let target = name(labels, goal.resolution.id);
        let Some(candidate) = candidates
            .iter()
            .find(|value| value.id == goal.resolution.id)
        else {
            return Err(Error::invalid_arguments_from(format!(
                "goal {} resolves to candidate {target}, which is not in this draft",
                name(labels, goal.id)
            )));
        };
        if !candidate.coverage_goal_ids.contains(&goal.id) {
            return Err(Error::invalid_arguments_from(format!(
                "goal {} resolves to candidate {target}; list it in that candidate's coverage_goals",
                name(labels, goal.id)
            )));
        }
    }
    for candidate in candidates {
        for goal_id in &candidate.coverage_goal_ids {
            let goal = goals
                .iter()
                .find(|value| value.id == *goal_id)
                .ok_or(Error::InvalidArguments)?;
            if goal.resolution.kind != CoverageResolutionKind::Candidate
                || goal.resolution.id != candidate.id
            {
                return Err(Error::invalid_arguments_from(format!(
                    "candidate {} lists goal {} in coverage_goals, but that goal resolves to {:?} {}; \
                     each goal is covered by exactly one candidate",
                    name(labels, candidate.id),
                    name(labels, goal.id),
                    goal.resolution.kind,
                    name(labels, goal.resolution.id)
                )));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_acyclic(candidates: &[CandidateEntity], labels: &Labels) -> Result<()> {
    let mut remaining: BTreeMap<_, BTreeSet<_>> = candidates
        .iter()
        .map(|value| (value.id, value.dependencies.iter().copied().collect()))
        .collect();
    loop {
        let ready: Vec<_> = remaining
            .iter()
            .filter(|(_, deps)| deps.is_empty())
            .map(|(id, _)| *id)
            .collect();
        if ready.is_empty() {
            break;
        }
        for id in &ready {
            remaining.remove(id);
        }
        for deps in remaining.values_mut() {
            for id in &ready {
                deps.remove(id);
            }
        }
    }
    if remaining.is_empty() {
        Ok(())
    } else {
        let cycle = remaining
            .keys()
            .map(|id| name(labels, *id))
            .collect::<Vec<_>>()
            .join(", ");
        Err(Error::invalid_arguments_from(format!(
            "candidate dependencies form a cycle among: {cycle}"
        )))
    }
}
