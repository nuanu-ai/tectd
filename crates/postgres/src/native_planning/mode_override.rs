//! Mode overrides in Slice planning: the user's permission behind the override of a
//! work node of a draft, and the two columns an opened Scope and an opened Slice carry.
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
        "SELECT i.sequence <= p.planning_latest_input, i.source_result_id IS NULL, \
                COALESCE(i.input, '') \
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

/// The override an opened Scope carries; `None` while its two columns are empty.
pub(super) async fn scope_override(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
) -> Result<Option<ScopeModeOverride>> {
    let carried: Option<(String, Uuid)> = sqlx::query_as(
        "SELECT mode_override, mode_override_source_ref_id FROM native_scopes \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND mode_override IS NOT NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    carried
        .map(|(mode, source_ref_id)| {
            Ok(ScopeModeOverride {
                mode: crate::programs::parse_mode(&mode)?,
                source_ref_id,
            })
        })
        .transpose()
}

/// The overrides the opened Slices of a Scope carry, by Slice.
pub(super) async fn slice_overrides(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
) -> Result<BTreeMap<Uuid, SliceModeOverride>> {
    let carried: Vec<(Uuid, String, Uuid)> = sqlx::query_as(
        "SELECT id, mode_override, mode_override_input_id FROM native_slices \
         WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3 AND mode_override IS NOT NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    carried.into_iter().map(carried_by_slice).collect()
}

/// The override one opened Slice carries; `None` while its two columns are empty.
pub(super) async fn slice_override(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    slice: Uuid,
) -> Result<Option<SliceModeOverride>> {
    let carried: Option<(Uuid, String, Uuid)> = sqlx::query_as(
        "SELECT id, mode_override, mode_override_input_id FROM native_slices \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND mode_override IS NOT NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(carried
        .map(carried_by_slice)
        .transpose()?
        .map(|(_, kept)| kept))
}

fn carried_by_slice(
    (slice, mode, input_id): (Uuid, String, Uuid),
) -> Result<(Uuid, SliceModeOverride)> {
    let mode = crate::programs::parse_mode(&mode)?;
    Ok((slice, SliceModeOverride { mode, input_id }))
}
