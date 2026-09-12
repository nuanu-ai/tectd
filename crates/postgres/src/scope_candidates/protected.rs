use super::resolve::{Kind, Source, resolve_ref};
use std::collections::{BTreeMap, BTreeSet};
use tect_domain::{
    CandidateEntity, Error, EvidenceEntity, EvidenceKind, ProtectedChangeDisposition,
    ProtectedChangeDraft, ProtectedChangeEntity, ResolvedCandidateDraft, Result,
    ScopeCandidateDraft,
};
use uuid::Uuid;

struct Resolution<'a> {
    sources: &'a BTreeMap<Uuid, Source>,
    handles: &'a BTreeMap<String, (Kind, Uuid)>,
    evidence_ids: &'a [(Uuid, i64)],
    candidate_ids: &'a [(Uuid, i64)],
    evidence: &'a [EvidenceEntity],
    candidates: &'a [CandidateEntity],
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_protected_changes(
    previous: Option<&ResolvedCandidateDraft>,
    draft: &ScopeCandidateDraft,
    sources: &BTreeMap<Uuid, Source>,
    handles: &BTreeMap<String, (Kind, Uuid)>,
    evidence_ids: &[(Uuid, i64)],
    candidate_ids: &[(Uuid, i64)],
    evidence: &[EvidenceEntity],
    candidates: &[CandidateEntity],
) -> Result<Vec<ProtectedChangeEntity>> {
    let Some(previous) = previous else {
        return if draft.protected_changes.is_empty() {
            Ok(Vec::new())
        } else {
            Err(Error::InvalidArguments)
        };
    };
    let state = Resolution {
        sources,
        handles,
        evidence_ids,
        candidate_ids,
        evidence,
        candidates,
    };
    let changes = draft
        .protected_changes
        .iter()
        .map(|change| {
            (
                (change.accepted_evidence_id, change.prior_candidate_id),
                change,
            )
        })
        .collect::<BTreeMap<_, _>>();
    if changes.len() != draft.protected_changes.len() {
        return Err(Error::InvalidArguments);
    }
    let mut used = BTreeSet::new();
    let mut resolved = Vec::new();
    for old in previous
        .evidence
        .iter()
        .filter(|value| value.kind == EvidenceKind::AcceptedWork)
    {
        let next = evidence.iter().find(|value| value.id == old.id);
        let same_evidence = next.is_some_and(|value| same_accepted(old, value, sources));
        if !same_evidence {
            let key = (old.id, None);
            let change = changes.get(&key).ok_or(Error::Forbidden)?;
            resolved.push(resolve_change(change, old, None, &state)?);
            used.insert(key);
        }
        for prior in previous
            .candidates
            .iter()
            .filter(|candidate| candidate.evidence_ids.contains(&old.id))
        {
            let same_edge = same_evidence
                && candidates
                    .iter()
                    .find(|candidate| candidate.id == prior.id)
                    .is_some_and(|candidate| candidate.evidence_ids.contains(&old.id));
            if !same_edge {
                let key = (old.id, Some(prior.id));
                let change = changes.get(&key).ok_or(Error::Forbidden)?;
                resolved.push(resolve_change(change, old, Some(prior), &state)?);
                used.insert(key);
            }
        }
    }
    if used.len() != changes.len() {
        return Err(Error::InvalidArguments);
    }
    Ok(resolved)
}

fn resolve_change(
    change: &ProtectedChangeDraft,
    old: &EvidenceEntity,
    prior: Option<&CandidateEntity>,
    state: &Resolution<'_>,
) -> Result<ProtectedChangeEntity> {
    let authority = state
        .sources
        .get(&change.authority_source_ref_id)
        .ok_or(Error::InvalidArguments)?;
    let old_authority = old
        .authority_input_sequence
        .ok_or(Error::InternalInvariant)?;
    if authority.kind != "planning_input"
        || authority
            .input_sequence
            .is_none_or(|sequence| sequence <= old_authority)
    {
        return Err(Error::Forbidden);
    }
    let target_candidate_id = change
        .target_candidate
        .as_ref()
        .map(|reference| {
            resolve_ref(
                reference,
                state.handles,
                Kind::Candidate,
                state.candidate_ids,
            )
        })
        .transpose()?;
    let replacement_evidence_id = change
        .replacement_evidence
        .as_ref()
        .map(|reference| resolve_ref(reference, state.handles, Kind::Evidence, state.evidence_ids))
        .transpose()?;
    let next = state.evidence.iter().find(|value| value.id == old.id);
    let prior_keeps_old = prior.is_some_and(|prior| {
        state
            .candidates
            .iter()
            .find(|candidate| candidate.id == prior.id)
            .is_some_and(|candidate| candidate.evidence_ids.contains(&old.id))
    });
    let valid = match (change.disposition, prior) {
        (ProtectedChangeDisposition::Delete, None) => next.is_none(),
        (ProtectedChangeDisposition::Delete, Some(_)) => !prior_keeps_old,
        (ProtectedChangeDisposition::Reassociate, Some(_)) => {
            next.is_some_and(|value| same_accepted(old, value, state.sources))
                && !prior_keeps_old
                && linked(target_candidate_id, old.id, state.candidates)
        }
        (ProtectedChangeDisposition::Replace, None) => {
            next.is_none()
                && valid_replacement(replacement_evidence_id, authority.input_sequence, state)
        }
        (ProtectedChangeDisposition::Replace, Some(_)) => {
            !prior_keeps_old
                && valid_replacement(replacement_evidence_id, authority.input_sequence, state)
                && target_candidate_id.is_some_and(|candidate| {
                    replacement_evidence_id
                        .is_some_and(|evidence| linked(Some(candidate), evidence, state.candidates))
                })
        }
        (ProtectedChangeDisposition::Reassociate, None) => false,
    };
    if !valid {
        return Err(Error::Forbidden);
    }
    Ok(ProtectedChangeEntity {
        accepted_evidence_id: old.id,
        prior_candidate_id: prior.map(|candidate| candidate.id),
        disposition: change.disposition,
        rationale: change.rationale.clone(),
        authority_source_ref_id: change.authority_source_ref_id,
        replacement_evidence_id,
        target_candidate_id,
    })
}

fn valid_replacement(
    id: Option<Uuid>,
    authority_sequence: Option<i64>,
    state: &Resolution<'_>,
) -> bool {
    id.and_then(|id| state.evidence.iter().find(|value| value.id == id))
        .is_some_and(|value| {
            value.kind == EvidenceKind::AcceptedWork
                && value.authority_input_sequence == authority_sequence
        })
}

fn linked(candidate_id: Option<Uuid>, evidence_id: Uuid, candidates: &[CandidateEntity]) -> bool {
    candidate_id
        .and_then(|id| candidates.iter().find(|candidate| candidate.id == id))
        .is_some_and(|candidate| candidate.evidence_ids.contains(&evidence_id))
}

fn same_accepted(
    old: &EvidenceEntity,
    next: &EvidenceEntity,
    sources: &BTreeMap<Uuid, Source>,
) -> bool {
    next.kind == old.kind
        && next.summary == old.summary
        && next.authority_input_sequence == old.authority_input_sequence
        && source_equivalent(old.source_ref_id, next.source_ref_id, sources)
}

fn source_equivalent(left: Uuid, right: Uuid, sources: &BTreeMap<Uuid, Source>) -> bool {
    let Some(left) = sources.get(&left) else {
        return false;
    };
    let Some(right) = sources.get(&right) else {
        return false;
    };
    left.kind == right.kind
        && left.input_sequence == right.input_sequence
        && left.program_field == right.program_field
        && left.body == right.body
}
