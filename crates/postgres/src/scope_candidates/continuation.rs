use super::resolve::{Kind, Source, resolve_ref};
use std::collections::{BTreeMap, BTreeSet};
use tect_domain::{
    BlockerEntity, CandidateAdded, CandidateChanged, CandidateDelta, CandidateEntity,
    CandidateSuperseded, CandidateUnchanged, CoverageGoalEntity, Error, EvidenceEntity,
    ResolvedCandidateDraft, Result, ScopeCandidateDraft,
};
use uuid::Uuid;

fn same_source(left: Uuid, right: Uuid, sources: &BTreeMap<Uuid, Source>) -> bool {
    match (sources.get(&left), sources.get(&right)) {
        (Some(left), Some(right)) => {
            left.kind == right.kind
                && left.input_sequence == right.input_sequence
                && left.program_field == right.program_field
                && left.body == right.body
        }
        _ => false,
    }
}

pub(super) fn stabilize_goals(
    current: &mut [CoverageGoalEntity],
    previous: Option<&ResolvedCandidateDraft>,
    sources: &BTreeMap<Uuid, Source>,
) -> Result<()> {
    for value in current {
        let Some(old) =
            previous.and_then(|draft| draft.goals.iter().find(|old| old.id == value.id))
        else {
            continue;
        };
        let unchanged = value.text == old.text
            && value.exact_quote == old.exact_quote
            && value.resolution == old.resolution
            && same_source(value.source_ref_id, old.source_ref_id, sources);
        value.revision = next_entity_revision(old.revision, unchanged)?;
    }
    Ok(())
}

pub(super) fn stabilize_evidence(
    current: &mut [EvidenceEntity],
    previous: Option<&ResolvedCandidateDraft>,
    sources: &BTreeMap<Uuid, Source>,
) -> Result<()> {
    for value in current {
        let Some(old) =
            previous.and_then(|draft| draft.evidence.iter().find(|old| old.id == value.id))
        else {
            continue;
        };
        let unchanged = value.kind == old.kind
            && value.summary == old.summary
            && value.authority_input_sequence == old.authority_input_sequence
            && same_source(value.source_ref_id, old.source_ref_id, sources);
        value.revision = next_entity_revision(old.revision, unchanged)?;
    }
    Ok(())
}

pub(super) fn stabilize_blockers(
    current: &mut [BlockerEntity],
    previous: Option<&ResolvedCandidateDraft>,
    sources: &BTreeMap<Uuid, Source>,
) -> Result<()> {
    for value in current {
        let Some(old) =
            previous.and_then(|draft| draft.blockers.iter().find(|old| old.id == value.id))
        else {
            continue;
        };
        let unchanged = value.summary == old.summary
            && same_source(value.source_ref_id, old.source_ref_id, sources);
        value.revision = next_entity_revision(old.revision, unchanged)?;
    }
    Ok(())
}

pub(super) fn stabilize_candidates(
    current: &mut [CandidateEntity],
    previous: Option<&ResolvedCandidateDraft>,
) -> Result<()> {
    for value in current {
        let Some(old) =
            previous.and_then(|draft| draft.candidates.iter().find(|old| old.id == value.id))
        else {
            continue;
        };
        value.revision = old.revision;
        let unchanged = value == old;
        value.revision = next_entity_revision(old.revision, unchanged)?;
    }
    Ok(())
}

fn next_entity_revision(previous: i64, unchanged: bool) -> Result<i64> {
    if unchanged {
        Ok(previous)
    } else {
        previous.checked_add(1).ok_or(Error::StorageUnavailable)
    }
}

pub(super) fn candidate_delta(
    draft: &ScopeCandidateDraft,
    previous: Option<&ResolvedCandidateDraft>,
    candidates: &[CandidateEntity],
    handles: &BTreeMap<String, (Kind, Uuid)>,
    candidate_ids: &[(Uuid, i64)],
) -> Result<CandidateDelta> {
    let Some(previous) = previous else {
        if !draft.supersessions.is_empty()
            || draft
                .candidates
                .iter()
                .any(|candidate| candidate.change_rationale.is_some())
        {
            return Err(Error::InvalidArguments);
        }
        return Ok(CandidateDelta {
            added: candidates
                .iter()
                .map(|candidate| CandidateAdded {
                    candidate_id: candidate.id,
                    revision: candidate.revision,
                })
                .collect(),
            ..CandidateDelta::default()
        });
    };
    let mut delta = CandidateDelta::default();
    for (input, candidate) in draft.candidates.iter().zip(candidates) {
        let old = previous
            .candidates
            .iter()
            .find(|old| old.id == candidate.id);
        match old {
            None => {
                if input.identity.local.is_none() || input.change_rationale.is_some() {
                    return Err(Error::InvalidArguments);
                }
                delta.added.push(CandidateAdded {
                    candidate_id: candidate.id,
                    revision: candidate.revision,
                });
            }
            Some(old) if old.revision == candidate.revision => {
                if input.change_rationale.is_some() {
                    return Err(Error::InvalidArguments);
                }
                delta.unchanged.push(CandidateUnchanged {
                    candidate_id: candidate.id,
                    revision: candidate.revision,
                });
            }
            Some(old) => {
                let rationale = input
                    .change_rationale
                    .as_ref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or(Error::InvalidArguments)?;
                delta.changed.push(CandidateChanged {
                    candidate_id: candidate.id,
                    from_revision: old.revision,
                    to_revision: candidate.revision,
                    rationale: rationale.clone(),
                });
            }
        }
    }
    let current_ids = candidates
        .iter()
        .map(|candidate| candidate.id)
        .collect::<BTreeSet<_>>();
    let omitted = previous
        .candidates
        .iter()
        .filter(|candidate| !current_ids.contains(&candidate.id))
        .collect::<Vec<_>>();
    if omitted.len() != draft.supersessions.len() {
        return Err(Error::InvalidArguments);
    }
    for supplied in &draft.supersessions {
        let prior = omitted
            .iter()
            .find(|candidate| candidate.id == supplied.candidate_id)
            .ok_or(Error::InvalidArguments)?;
        if supplied.revision != prior.revision {
            return Err(Error::StaleRevision);
        }
        let replacements = supplied
            .replacements
            .iter()
            .map(|reference| resolve_ref(reference, handles, Kind::Candidate, candidate_ids))
            .collect::<Result<Vec<_>>>()?;
        delta.superseded.push(CandidateSuperseded {
            prior: (*prior).clone(),
            reason: supplied.reason.clone(),
            replacement_candidate_ids: replacements,
        });
    }
    Ok(delta)
}
