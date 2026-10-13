//! Which candidates of a draft its save created or changed. `sizing` and `pipeline`
//! ask Jev about these and no others, and each is named with the id and the revision
//! the store holds it under, since that is what Jev's answer about it is kept for.
use crate::{
    Error, ResolvedCandidateDraft, ResolvedSliceCandidateDraft, Result, ScopeCandidateDraft,
    SliceCandidateDraft, SliceCandidateDraftNode,
};
use uuid::Uuid;

/// A candidate a save wrote: its 1-based draft position, counted over every node of
/// the draft, and the id and revision the store holds it under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Written {
    pub position: usize,
    pub id: Uuid,
    pub revision: i64,
}

/// The candidates of a Scope candidate draft that its save wrote, in draft order,
/// given the draft the save `stored`.
pub(super) fn scope_candidates(
    draft: &ScopeCandidateDraft,
    stored: &ResolvedCandidateDraft,
) -> Result<Vec<Written>> {
    let named = draft.candidates.iter().map(|candidate| {
        let identity = &candidate.identity;
        Some((identity.local.is_some(), identity.id, identity.revision))
    });
    let kept = stored
        .candidates
        .iter()
        .map(|kept| (kept.id, kept.revision));
    written(named.collect(), kept.collect())
}

/// The work nodes of a Slice candidate draft that its save wrote, in draft order,
/// given the draft the save `stored`. A decision node is never among them.
pub(super) fn slice_work_nodes(
    draft: &SliceCandidateDraft,
    stored: &ResolvedSliceCandidateDraft,
) -> Result<Vec<Written>> {
    let named = draft.nodes.iter().map(|node| match node {
        SliceCandidateDraftNode::Work { identity, .. } => Some((
            identity.local.is_some(),
            identity.candidate_id,
            identity.revision,
        )),
        SliceCandidateDraftNode::Decision { .. } => None,
    });
    let kept = stored.nodes.iter().map(|kept| (kept.id(), kept.revision()));
    written(named.collect(), kept.collect())
}

/// How a draft names one candidate that may be asked about: whether by a local
/// label, and the id and revision it names otherwise. `None` is a node nobody asks about.
type Named = Option<(bool, Option<Uuid>, Option<i64>)>;

/// The one rule. A candidate the draft names by a local label is new. A candidate it
/// names by id is changed when the store holds it under a higher revision than the
/// draft named, and unchanged when the revision is the one named.
fn written(named: Vec<Named>, kept: Vec<(Uuid, i64)>) -> Result<Vec<Written>> {
    // The save has stored every candidate of the draft, in draft order.
    if named.len() != kept.len() {
        return Err(Error::InternalInvariant);
    }
    let mut written = Vec::new();
    for (index, (named, (id, revision))) in named.into_iter().zip(kept).enumerate() {
        let wrote = match named {
            None => false,
            Some((true, _, _)) => true,
            // The save has held that a candidate without a label names an id and a
            // revision, and has stored it under that id.
            Some((false, Some(named_id), Some(named_revision))) if named_id == id => {
                revision > named_revision
            }
            Some(_) => return Err(Error::InternalInvariant),
        };
        if wrote {
            written.push(Written {
                position: index + 1,
                id,
                revision,
            });
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests;
