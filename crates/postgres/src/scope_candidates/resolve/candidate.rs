use super::*;

pub(super) fn candidate(
    value: &CandidateDraft,
    id: Uuid,
    revision: i64,
    handles: &BTreeMap<String, (Kind, Uuid)>,
    candidates: &[(Uuid, i64)],
    goals: &[(Uuid, i64)],
    evidence: &[(Uuid, i64)],
) -> Result<CandidateEntity> {
    Ok(CandidateEntity {
        id,
        revision,
        grounding: value.grounding,
        title: value.title.clone(),
        outcome: value.outcome.clone(),
        trigger: value.trigger.clone(),
        delivered_behavior: value.delivered_behavior.clone(),
        proof: value.proof.clone(),
        includes: value.includes.clone(),
        excludes: value.excludes.clone(),
        dependencies: value
            .dependencies
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Candidate, candidates))
            .collect::<Result<_>>()?,
        coverage_goal_ids: value
            .coverage_goals
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Goal, goals))
            .collect::<Result<_>>()?,
        evidence_ids: value
            .evidence
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Evidence, evidence))
            .collect::<Result<_>>()?,
    })
}
