//! Mode overrides in Slice planning: the user's permission behind the override of a
//! work node of a draft.
use super::*;

/// The override the node at `index` of a draft may hold. A node that states none
/// holds none. One that states one keeps it when its `input_id` is an input the user
/// gave to this Scope's Slice candidate set, held by the set's current snapshot, and
/// that input's text contains the quote; anything else is the refusal.
pub(super) async fn permit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
    index: usize,
    node: &SliceCandidateDraftNode,
) -> Result<Option<Box<SliceCandidateModeOverride>>> {
    let SliceCandidateDraftNode::Work {
        mode_override: Some(stated),
        ..
    } = node
    else {
        return Ok(None);
    };
    // An input a Slice result produced carries `source_result_id`: the user did not
    // write it. The save has already held that the draft names the current snapshot.
    let input: Option<(bool, bool, String)> = sqlx::query_as(
        "SELECT i.sequence <= p.planning_latest_input, i.source_result_id IS NULL, i.input \
         FROM slice_candidate_sets s \
         JOIN slice_planning_snapshots p ON p.tenant_id=s.tenant_id \
          AND p.workspace_id=s.workspace_id AND p.candidate_set_id=s.id \
          AND p.id=s.current_snapshot_id \
         JOIN slice_planning_inputs i ON i.tenant_id=s.tenant_id \
          AND i.workspace_id=s.workspace_id AND i.candidate_set_id=s.id \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.scope_id=$3 AND i.id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope)
    .bind(stated.input_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let record = match &input {
        None => PermissionRecord::Unknown,
        Some((false, _, _)) => PermissionRecord::OutsideSnapshot,
        Some((true, false, _)) => PermissionRecord::NotUserInput,
        Some((true, true, text)) => PermissionRecord::UserInput(text),
    };
    Ok(Some(Box::new(stated.permitted(index, record)?)))
}
