use super::*;

mod delivery;
mod generic;
mod inquiry;
mod legacy;
mod legacy_capture;
#[allow(dead_code)] // The host page adapter is a later packet.
mod paged_reader;
mod paged_writer;
pub(crate) use delivery::{
    authorize_manifest, authorize_owned_copy, load, load_resources, resource_status,
    resource_status_from_capture,
};
#[cfg(test)]
use legacy_capture::unique_legacy_items;
pub(crate) use legacy_capture::{Captured, capture_with_snapshot, status};
use legacy_capture::{items, preview, projection};
#[allow(unused_imports)] // Re-exported for the later page adapter.
pub(crate) use paged_reader::{read_manifest_resource_page, read_pinned_resource};
pub(crate) use paged_writer::{PagedCapture, PagedCaptured, capture_paged};

#[allow(clippy::too_many_arguments)]
pub(crate) async fn paged_resource_status_from_capture(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: Option<&str>,
    manifest: &PagedPipelineKnowledgeManifest,
    pins: &[PagedPipelineKnowledgeResourcePin],
    captured: &PagedCaptured,
) -> Result<Option<PipelineKnowledgeResourceStatus>> {
    let Some(phase) = phase else { return Ok(None) };
    let state: Option<(i64, bool, i64)> = sqlx::query_as("SELECT k.generation,k.capability_ready,r.revision FROM workspace_knowledge_state k JOIN slice_pipeline_runs r ON r.tenant_id=k.tenant_id AND r.workspace_id=k.workspace_id WHERE k.tenant_id=$1 AND k.workspace_id=$2 AND r.id=$3 AND r.scope_id=$4 AND r.slice_id=$5")
        .bind(tenant).bind(workspace).bind(run).bind(scope).bind(slice)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((generation, ready, run_revision)) = state else {
        return Ok(None);
    };
    let snapshot = &captured.snapshot;
    if !ready
        || snapshot.manifest.phase_id != phase
        || snapshot.manifest.run_id != run
        || snapshot.manifest.run_revision != run_revision
        || snapshot.manifest.workspace_generation != generation
        || snapshot.manifest.id != manifest.id
        || snapshot.manifest.semantic_digest != manifest.semantic_digest
        || snapshot.manifest.selected.len() != pins.len()
        || captured.principal != principal
        || captured.manifest != *manifest
        || captured.pins != pins
    {
        return paged_resource_status(
            tx,
            tenant,
            workspace,
            principal,
            run,
            scope,
            slice,
            Some(phase),
            manifest,
            pins,
        )
        .await;
    }
    delivery::require_identity_ready(tx).await?;
    // The workspace state lock excludes knowledge writes, while wall clock
    // validity and review boundaries can still advance during this transaction.
    let crossed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_catalog.unnest($1::text[]) AS boundary(value) WHERE boundary.value::timestamptz >= $2::timestamptz AND boundary.value::timestamptz <= pg_catalog.clock_timestamp())")
        .bind(&snapshot.temporal_boundaries).bind(&snapshot.captured_before)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if crossed {
        return paged_resource_status(
            tx,
            tenant,
            workspace,
            principal,
            run,
            scope,
            slice,
            Some(phase),
            manifest,
            pins,
        )
        .await;
    }
    Ok(Some(delivery::project_paged_resource_status(
        generation,
        run_revision,
        manifest,
        pins,
        snapshot,
    )))
}

pub(crate) async fn load_paged_resources(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    id: Option<Uuid>,
    principal: Uuid,
) -> Result<
    Option<(
        PagedPipelineKnowledgeManifest,
        Vec<PagedPipelineKnowledgeResourcePin>,
    )>,
> {
    let Some(id) = id else { return Ok(None) };
    let Some(contract) = delivery::authorize_manifest(tx, tenant, workspace, id, principal).await?
    else {
        return Ok(None);
    };
    if contract != PAGED_KNOWLEDGE_CONTRACT_VERSION {
        return Ok(None);
    }
    let digest: String = sqlx::query_scalar("SELECT digest FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT payload_erased")
        .bind(tenant).bind(workspace).bind(id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    Ok(Some(
        paged_reader::load_manifest_commitment(tx, tenant, workspace, id, &digest).await?,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn paged_resource_status(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: Option<&str>,
    manifest: &PagedPipelineKnowledgeManifest,
    pins: &[PagedPipelineKnowledgeResourcePin],
) -> Result<Option<PipelineKnowledgeResourceStatus>> {
    let Some(phase) = phase else { return Ok(None) };
    let state: Option<(i64, bool, i64)> = sqlx::query_as("SELECT k.generation,k.capability_ready,r.revision FROM workspace_knowledge_state k JOIN slice_pipeline_runs r ON r.tenant_id=k.tenant_id AND r.workspace_id=k.workspace_id WHERE k.tenant_id=$1 AND k.workspace_id=$2 AND r.id=$3 AND r.scope_id=$4 AND r.slice_id=$5")
        .bind(tenant).bind(workspace).bind(run).bind(scope).bind(slice)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((generation, ready, run_revision)) = state else {
        return Ok(None);
    };
    if !ready {
        return Ok(Some(PipelineKnowledgeResourceStatus {
            state: PipelineKnowledgeResourceState::Inactive,
            current_generation: generation,
            changed_unit_ids: Vec::new(),
            freshness_warnings: Vec::new(),
            access_changed: false,
        }));
    }
    // The compact wire shape does not change the status policy. Recompute the
    // complete current projection internally, including selector gaps and time
    // boundaries, then compare its basis with the committed header and pins.
    let current = generic::snapshot(
        tx,
        tenant,
        workspace,
        principal,
        run,
        run_revision,
        scope,
        slice,
        phase,
        manifest.id,
        manifest.digest.clone(),
    )
    .await?;
    Ok(Some(delivery::project_paged_resource_status(
        generation,
        run_revision,
        manifest,
        pins,
        &current,
    )))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn validate_completion(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    manifest: Option<&PipelineKnowledgeManifest>,
    consumed: Option<&ConsumedKnowledgeManifestRef>,
) -> Result<()> {
    let principal: Option<Uuid> = sqlx::query_scalar("SELECT tect_dk_session_principal($1)")
        .bind(session)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let principal = principal.ok_or(Error::Forbidden)?;
    let (generation, ready, current, _, gaps) =
        preview(tx, tenant, workspace, run, scope, slice, phase, principal).await?;
    if !ready {
        return if manifest.is_none() && consumed.is_none() {
            Ok(())
        } else {
            Err(Error::ContextChanged)
        };
    }
    let Some(manifest) = manifest else {
        return Err(Error::NeedsContext);
    };
    let current_run_revision: i64 = sqlx::query_scalar("SELECT revision FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND scope_id=$4 AND slice_id=$5")
        .bind(tenant).bind(workspace).bind(run).bind(scope).bind(slice)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    let paged = load_paged_resources(tx, tenant, workspace, Some(manifest.id), principal).await?;
    let resources = if paged.is_none() {
        load_resources(tx, tenant, workspace, Some(manifest.id), principal).await?
    } else {
        None
    };
    let current_resources = generic::snapshot(
        tx,
        tenant,
        workspace,
        principal,
        run,
        current_run_revision,
        scope,
        slice,
        phase,
        manifest.id,
        manifest.digest.clone(),
    )
    .await?;
    if !gaps.is_empty() {
        return Err(Error::NeedsContext);
    }
    if !current_resources.blocking_gaps.is_empty() {
        return Err(Error::NeedsContext);
    }
    if manifest.workspace_generation != generation || manifest.semantic_digest != current {
        return Err(Error::ContextChanged);
    }
    if let Some((header, pins)) = paged.as_ref() {
        // The legacy selection and the paged resource selection have separate
        // semantic digests. The latter commits the compact header and every
        // ordered child, independently of which pages a caller has read.
        if header.id != manifest.id
            || header.digest != manifest.digest
            || header.run_id != run
            || header.run_revision != current_run_revision
            || header.phase_id != phase
        {
            return Err(Error::ContextChanged);
        }
        let status = delivery::project_paged_resource_status(
            generation,
            current_run_revision,
            header,
            pins,
            &current_resources,
        );
        if status.access_changed || status.state != PipelineKnowledgeResourceState::Current {
            return match status.state {
                PipelineKnowledgeResourceState::NeedsContext => Err(Error::NeedsContext),
                _ => Err(Error::ContextChanged),
            };
        }
        return if manifest.selected.is_empty() && pins.is_empty() {
            if consumed.is_some() {
                Err(Error::StaleContext)
            } else {
                Ok(())
            }
        } else if consumed.is_some_and(|v| v.manifest_id == header.id && v.digest == header.digest)
        {
            Ok(())
        } else {
            Err(Error::NeedsContext)
        };
    }
    let Some(resources) = resources.as_ref() else {
        return Err(Error::ContextChanged);
    };
    let resource_current = {
        let stored = resources;
        stored.workspace_generation == generation
            && stored.run_revision == current_run_revision
            && stored.digest == manifest.digest
            && stored.semantic_digest == current_resources.manifest.semantic_digest
            && stored.definition_version == current_resources.manifest.definition_version
            && stored.definition_digest == current_resources.manifest.definition_digest
            && stored.method_requirements == current_resources.manifest.method_requirements
            && stored.inquiry == current_resources.manifest.inquiry
            && stored.projection_policy == current_resources.manifest.projection_policy
    };
    if !resource_current {
        return Err(Error::ContextChanged);
    }
    let resource_selected = !resources.selected.is_empty();
    if manifest.selected.is_empty() && !resource_selected {
        if consumed.is_some() {
            return Err(Error::StaleContext);
        };
        Ok(())
    } else if consumed.is_some_and(|v| v.manifest_id == manifest.id && v.digest == manifest.digest)
    {
        Ok(())
    } else {
        Err(Error::NeedsContext)
    }
}

pub(crate) async fn refresh(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &RefreshPipelineKnowledge,
) -> Result<RefreshPipelineKnowledgeOutcome> {
    let payload = json(request)?;
    if let Some(prior) = receipt::<RefreshPipelineKnowledgeOutcome>(
        tx,
        tenant,
        workspace,
        "refresh",
        request.request_id,
        &payload,
    )
    .await?
    {
        return refresh_replay(tx, tenant, workspace, session, request.run_id, prior).await;
    }
    let (_, ready, _) = lock_state(tx, tenant, workspace).await?;
    if !ready {
        return Err(Error::KnowledgeUnavailable);
    }
    if let Some(prior) = receipt::<RefreshPipelineKnowledgeOutcome>(
        tx,
        tenant,
        workspace,
        "refresh",
        request.request_id,
        &payload,
    )
    .await?
    {
        return refresh_replay(tx, tenant, workspace, session, request.run_id, prior).await;
    }
    let row:Option<(Uuid,Uuid,i64,Option<String>)>=sqlx::query_as("SELECT scope_id,slice_id,revision,current_phase_id FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let (scope, slice, revision, current) = row.ok_or(Error::NotFound)?;
    if revision != request.run_revision || current.as_deref() != Some(&request.phase_id) {
        return Err(Error::StaleRevision);
    }
    let next = revision.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query("UPDATE slice_pipeline_runs SET revision=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).bind(next).execute(&mut **tx).await.map_err(storage_error)?;
    let captured = capture_paged(
        tx,
        tenant,
        workspace,
        request.run_id,
        next,
        scope,
        slice,
        &request.phase_id,
        session,
    )
    .await?
    .ok_or(Error::KnowledgeUnavailable)?;
    let (id, digest, outcome) = match captured {
        PagedCapture::Paged(value) => {
            let value = *value;
            if !value.manifest.unresolved_needs.is_empty() {
                return Err(Error::NeedsContext);
            }
            (
                value.manifest.id,
                value.manifest.digest.clone(),
                RefreshPipelineKnowledgeOutcome::PagedRefreshed(value.manifest),
            )
        }
        PagedCapture::Inline(value) => {
            let value = *value;
            let blocking: serde_json::Value = sqlx::query_scalar("SELECT resource_unresolved_needs FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
                .bind(tenant).bind(workspace).bind(value.manifest.id).fetch_one(&mut **tx).await.map_err(storage_error)?;
            if !decode::<Vec<String>>(blocking)?.is_empty() {
                return Err(Error::NeedsContext);
            }
            (
                value.manifest.id,
                value.manifest.digest.clone(),
                RefreshPipelineKnowledgeOutcome::Refreshed(value.manifest),
            )
        }
    };
    sqlx::query("UPDATE slice_pipeline_runs SET knowledge_manifest_id=$4,knowledge_manifest_digest=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).bind(id).bind(&digest).execute(&mut **tx).await.map_err(storage_error)?;
    save_receipt(
        tx,
        tenant,
        workspace,
        "refresh",
        request.request_id,
        session,
        &payload,
        &outcome,
    )
    .await?;
    Ok(outcome)
}

async fn refresh_replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    run: Uuid,
    prior: RefreshPipelineKnowledgeOutcome,
) -> Result<RefreshPipelineKnowledgeOutcome> {
    let principal: Uuid =
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT tect_dk_session_principal($1)")
            .bind(session)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?
            .ok_or(Error::Forbidden)?;
    let (id, expected_run) = match &prior {
        RefreshPipelineKnowledgeOutcome::Refreshed(v)
        | RefreshPipelineKnowledgeOutcome::Replay(v) => (v.id, v.run_id),
        RefreshPipelineKnowledgeOutcome::PagedRefreshed(v)
        | RefreshPipelineKnowledgeOutcome::PagedReplay(v) => (v.id, v.run_id),
    };
    if expected_run != run {
        return Err(Error::InternalInvariant);
    }
    authorize_manifest(tx, tenant, workspace, id, principal)
        .await?
        .ok_or(Error::Forbidden)?;
    match prior {
        RefreshPipelineKnowledgeOutcome::Refreshed(v)
        | RefreshPipelineKnowledgeOutcome::Replay(v) => {
            Ok(RefreshPipelineKnowledgeOutcome::Replay(v))
        }
        RefreshPipelineKnowledgeOutcome::PagedRefreshed(v)
        | RefreshPipelineKnowledgeOutcome::PagedReplay(v) => {
            let (current, _) = load_paged_resources(tx, tenant, workspace, Some(v.id), principal)
                .await?
                .ok_or(Error::InternalInvariant)?;
            if current != v {
                return Err(Error::InternalInvariant);
            }
            Ok(RefreshPipelineKnowledgeOutcome::PagedReplay(v))
        }
    }
}
