use super::*;

pub(super) fn open_scope_context_mut(
    outcome: &mut OpenScopeOutcome,
) -> &mut tect_domain::OpenScopeContext {
    match outcome {
        OpenScopeOutcome::Created(value) | OpenScopeOutcome::Replay(value) => value,
    }
}

pub(super) async fn ensure_slice_fresh(
    tx: &mut dyn UnitOfWork,
    workspace_id: uuid::Uuid,
    scope_id: uuid::Uuid,
    guidance: &dyn NativePlanningGuidance,
) -> Result<()> {
    let stored = tx
        .slice_candidate_context(workspace_id, scope_id)
        .await?
        .ok_or(Error::NotFound)?;
    let current = guidance.snapshot(
        &basis_from_context(&stored)?,
        &stored.inputs,
        &stored.results,
    )?;
    if slice_stale_reasons(&stored, &current).is_empty() {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

pub(super) fn basis_from_context(
    context: &SliceCandidateContext,
) -> Result<tect_domain::ScopeOpenBasis> {
    Ok(tect_domain::ScopeOpenBasis {
        boundary: context.scope.boundary,
        title: context.scope.title.clone(),
        outcome: context.scope.outcome.clone(),
        includes: context.scope.includes.clone(),
        excludes: context.scope.excludes.clone(),
        source_candidate_set_revision: context.snapshot.source_candidate_set_revision,
        source_snapshot_id: context.snapshot.source_snapshot_id,
    })
}

pub(super) fn validate_scope_open(r: &OpenScope) -> Result<()> {
    if r.request_id.is_nil()
        || r.candidate_set_id.is_nil()
        || r.candidate_set_revision < 1
        || r.candidate_snapshot_id.is_nil()
        || r.candidate_id.is_nil()
        || r.candidate_revision < 1
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
pub(super) fn validate_slice_write(
    scope: uuid::Uuid,
    set: uuid::Uuid,
    revision: i64,
    snapshot: uuid::Uuid,
    cursor: i64,
    request: uuid::Uuid,
) -> Result<()> {
    if scope.is_nil()
        || set.is_nil()
        || snapshot.is_nil()
        || request.is_nil()
        || revision < 1
        || cursor < 0
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
pub(super) fn validate_material(m: &tect_domain::SlicePlanningSnapshotMaterial) -> Result<()> {
    m.catalogue.validate()?;
    if m.method.id.trim().is_empty()
        || m.method.revision.trim().is_empty()
        || m.method.digest.trim().is_empty()
        || m.method.body.trim().is_empty()
        || m.registry_revision.trim().is_empty()
        || m.registry_digest.trim().is_empty()
        || m.rules.len() != 4
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
pub(super) fn validate_result(r: &RecordSliceResult) -> Result<()> {
    if r.request_id.is_nil()
        || r.scope_id.is_nil()
        || r.slice_id.is_nil()
        || r.slice_revision < 1
        || r.summary.trim().is_empty()
        || r.evidence.is_empty()
        || r.evidence.iter().any(|e| {
            e.kind.trim().is_empty()
                || e.reference.trim().is_empty()
                || e.observation.trim().is_empty()
        })
        || r.scope_impact.trim().is_empty()
        || r.remaining_work.trim().is_empty()
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
pub(super) fn slice_stale_reasons(
    c: &SliceCandidateContext,
    m: &tect_domain::SlicePlanningSnapshotMaterial,
) -> Vec<String> {
    let mut r = Vec::new();
    if c.snapshot.planning_latest_input != c.candidate_set.latest_input {
        r.push("planning_inputs".into())
    }
    if c.snapshot.method.revision != m.method.revision
        || c.snapshot.method.digest != m.method.digest
    {
        r.push("method".into())
    }
    if c.snapshot.registry_revision != m.registry_revision
        || c.snapshot.registry_digest != m.registry_digest
    {
        r.push("rules".into())
    }
    if c.snapshot.catalogue.revision != m.catalogue.revision
        || c.snapshot.catalogue.digest != m.catalogue.digest
    {
        r.push("pipeline_catalogue".into())
    }
    let mut captured = c.snapshot.result_ids.clone();
    captured.sort_unstable();
    let mut current = c.results.iter().map(|result| result.id).collect::<Vec<_>>();
    current.sort_unstable();
    if captured != current {
        r.push("slice_results".into())
    }
    r
}
