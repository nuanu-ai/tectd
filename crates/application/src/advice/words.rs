//! The user's recorded words behind a candidate set, by source and in recorded order.
//! Every advice point reads them through these two functions and in no other way.
use crate::AdviceStore;
use tect_domain::*;
use uuid::Uuid;

/// One sequence of inputs and the last entry of it that is read, inclusive.
pub(crate) type Through = (Uuid, i64);

/// Words behind a Scope candidate set: the inputs of its Program, then the set's own.
pub(crate) async fn scope_level(
    store: &mut dyn AdviceStore,
    workspace_id: Uuid,
    (program_id, program_through): Through,
    (candidate_set_id, planning_through): Through,
) -> Result<Vec<SizingWord>> {
    let program = store
        .program_input_texts(workspace_id, program_id, program_through)
        .await?;
    let planning = store
        .scope_candidate_input_texts(workspace_id, candidate_set_id, planning_through)
        .await?;
    let mut words = labelled(SizingWordSource::ProgramInput, program);
    words.extend(labelled(SizingWordSource::ScopeCandidateInput, planning));
    Ok(words)
}

/// Words behind the Slice candidate set of an opened Scope: what stood behind the
/// Scope candidate set `source_set_id` as `basis` had read it, then the Slice
/// candidate set's own inputs. Inputs a Slice result produced are not among them.
pub(crate) async fn slice_level(
    store: &mut dyn AdviceStore,
    workspace_id: Uuid,
    (source_set_id, basis): (Uuid, &SizingWordsBasis),
    (candidate_set_id, planning_through): Through,
) -> Result<Vec<SizingWord>> {
    let mut words = scope_level(
        store,
        workspace_id,
        (basis.program_id, basis.program_latest_input),
        (source_set_id, basis.planning_latest_input),
    )
    .await?;
    let planning = store
        .slice_candidate_input_texts(workspace_id, candidate_set_id, planning_through)
        .await?;
    words.extend(labelled(SizingWordSource::SliceCandidateInput, planning));
    Ok(words)
}

fn labelled(source: SizingWordSource, texts: Vec<String>) -> Vec<SizingWord> {
    texts
        .into_iter()
        .map(|text| SizingWord { source, text })
        .collect()
}
