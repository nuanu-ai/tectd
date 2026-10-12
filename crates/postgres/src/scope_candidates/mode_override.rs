//! The user's permission behind the mode override of a Scope candidate: what the
//! reference it cites is, among the references this candidate set has issued.
use super::resolve::Source;
use std::collections::BTreeMap;
use tect_domain::{CandidateEntity, PermissionRecord, Result, ScopeCandidateDraft};
use uuid::Uuid;

/// Only an input given to this candidate set is the user's permission. The Program's
/// fields are referenced too, and its inputs have no reference at all.
const USER_INPUT: &str = "planning_input";

fn record(
    sources: &BTreeMap<Uuid, Source>,
    snapshot_id: Uuid,
    source_ref_id: Uuid,
) -> PermissionRecord<'_> {
    match sources.get(&source_ref_id) {
        None => PermissionRecord::Unknown,
        Some(source) if source.snapshot_id != snapshot_id => PermissionRecord::OutsideSnapshot,
        Some(source) if source.kind != USER_INPUT => PermissionRecord::NotUserInput,
        Some(source) => PermissionRecord::UserInput(&source.body),
    }
}

/// Give each resolved candidate the override its draft states, once the reference it
/// cites is an input of the snapshot the draft is saved against and holds the quote.
pub(super) fn permit(
    candidates: &mut [CandidateEntity],
    draft: &ScopeCandidateDraft,
    sources: &BTreeMap<Uuid, Source>,
    snapshot_id: Uuid,
) -> Result<()> {
    for (index, (stated, candidate)) in draft.candidates.iter().zip(candidates).enumerate() {
        if let Some(stated) = &stated.mode_override {
            let record = record(sources, snapshot_id, stated.source_ref_id);
            candidate.mode_override = Some(stated.permitted(index, record)?);
        }
    }
    Ok(())
}

/// Whether two overrides are one permission: the same level and quote, cited through
/// references that different snapshots issued for the same input.
pub(super) fn same_permission(
    candidate: &CandidateEntity,
    old: &CandidateEntity,
    sources: &BTreeMap<Uuid, Source>,
) -> bool {
    match (&candidate.mode_override, &old.mode_override) {
        (None, None) => true,
        (Some(now), Some(then)) => {
            now.mode == then.mode
                && now.exact_quote == then.exact_quote
                && super::continuation::same_source(now.source_ref_id, then.source_ref_id, sources)
        }
        _ => false,
    }
}
