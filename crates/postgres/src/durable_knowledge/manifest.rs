use super::*;
use std::collections::{BTreeMap, BTreeSet};

mod delivery;
mod generic;
mod inquiry;
mod legacy;
#[allow(dead_code)] // The host page adapter is a later packet.
mod paged_reader;
mod paged_writer;
pub(crate) use delivery::{
    authorize_manifest, authorize_owned_copy, load, load_resources, resource_status,
    resource_status_from_capture,
};
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
async fn projection(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    principal: Uuid,
) -> Result<(Vec<KnowledgeUnitRevision>, Vec<String>)> {
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let rows:Vec<(Uuid,bool,bool,bool,bool)>=sqlx::query_as("SELECT h.unit_id,(h.active AND b.active),(b.binding_kind='workspace' OR (b.definition_kind=run.definition_kind AND b.definition_version=run.definition_version AND b.definition_digest=run.definition_digest)),(h.access_scope='owners_only' OR revision.access_scope='owners_only'),(h.payload_erased OR revision.payload_erased) FROM knowledge_unit_heads h JOIN knowledge_bindings b ON b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id AND b.unit_id=h.unit_id AND b.revision=h.accepted_revision JOIN knowledge_revisions revision ON revision.tenant_id=h.tenant_id AND revision.workspace_id=h.workspace_id AND revision.unit_id=h.unit_id AND revision.revision=h.accepted_revision JOIN slice_pipeline_runs run ON run.tenant_id=h.tenant_id AND run.workspace_id=h.workspace_id AND run.id=$3 WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.contract_version='dk-1' AND (b.binding_kind='workspace' OR (b.binding_kind='slice_phase' AND b.scope_id=$4 AND b.slice_id=$5 AND b.phase_id=$6)) ORDER BY h.unit_id")
        .bind(tenant).bind(workspace).bind(run).bind(scope).bind(slice).bind(phase).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut active = Vec::new();
    let mut gaps = Vec::new();
    for (unit, enabled, pin_matches, restricted, erased) in rows {
        if restricted && !owner {
            gaps.push("resource_inaccessible".into());
        } else if erased {
            gaps.push("resource_unavailable".into());
        } else if enabled && pin_matches {
            active.push(
                context::load_revision(tx, tenant, workspace, unit, None, false)
                    .await?
                    .ok_or(Error::InternalInvariant)?,
            );
        } else if !enabled {
            gaps.push(format!("required_binding_withdrawn:{unit}"));
        } else {
            gaps.push(format!("binding_definition_changed:{unit}"));
        }
    }
    Ok((active, gaps))
}

fn items(revisions: &[KnowledgeUnitRevision]) -> Vec<PipelineKnowledgeItem> {
    let selected = revisions
        .iter()
        .map(|value| PipelineKnowledgeItem {
            unit_id: value.unit_id,
            revision: value.revision,
            rdf_digest: value.rdf_digest.clone(),
            source_sha256: value.source_sha256.clone(),
            why_included: match value.constraint.binding {
                KnowledgeBinding::Workspace => "workspace_binding".into(),
                KnowledgeBinding::SlicePhase { .. } => "exact_slice_phase_binding".into(),
            },
            unit_iri: value.unit_iri.clone(),
            revision_iri: value.revision_iri.clone(),
            source_iri: value.source_iri.clone(),
            source_uri: value.constraint.source.uri.clone(),
            title: value.constraint.title.clone(),
            statement: value.constraint.statement.clone(),
            modality: value.constraint.modality,
            action: value.constraint.action.clone(),
            target_iri: value.constraint.target_iri.clone(),
            conditions: value.constraint.conditions.clone(),
            exceptions: value.constraint.exceptions.clone(),
        })
        .collect();
    unique_legacy_items(selected)
}

fn unique_legacy_items(selected: Vec<PipelineKnowledgeItem>) -> Vec<PipelineKnowledgeItem> {
    let mut seen = BTreeSet::new();
    selected
        .into_iter()
        .filter(|item| seen.insert((item.unit_id, item.revision, item.rdf_digest.clone())))
        .collect()
}

pub(crate) struct Captured {
    pub manifest: PipelineKnowledgeManifest,
    snapshot: generic::Snapshot,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn capture_with_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    run_revision: i64,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
) -> Result<Option<Captured>> {
    let state:Option<(i64,bool)>=sqlx::query_as("SELECT generation,capability_ready FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2").bind(tenant).bind(workspace).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((generation, true)) = state else {
        return Ok(None);
    };
    delivery::require_identity_ready(tx).await?;
    let principal: Option<Uuid> = sqlx::query_scalar("SELECT tect_dk_session_principal($1)")
        .bind(session)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let principal = principal.ok_or(Error::Forbidden)?;
    let inquiry_projection = inquiry::load(tx, tenant, workspace, run).await?;
    let (revisions, unresolved) = if inquiry_projection
        .as_ref()
        .and_then(|value| value.stage())
        .is_some()
    {
        (Vec::new(), Vec::new())
    } else {
        projection(tx, tenant, workspace, run, scope, slice, phase, principal).await?
    };
    for value in &revisions {
        let rows = rdf::native_rows(
            tx,
            tenant,
            workspace,
            value.unit_id,
            value.revision,
            value.publication_event_id,
        )
        .await?;
        rdf::validate_rows(&rows, value)?;
    }
    let selected = items(&revisions);
    if serde_json::to_vec(&(&selected, &unresolved))
        .map_err(storage_error)?
        .len()
        > DK_MAX_MANIFEST_BYTES
    {
        return Err(Error::CapacityExceeded);
    }
    let semantic_digest = legacy::semantic(&selected, &unresolved)?;
    let id = Uuid::new_v4();
    let mut snapshot = generic::snapshot(
        tx,
        tenant,
        workspace,
        principal,
        run,
        run_revision,
        scope,
        slice,
        phase,
        id,
        String::new(),
    )
    .await?;
    let resource = &mut snapshot.manifest;
    let base_digest = digest(&(
        id,
        generation,
        run,
        run_revision,
        phase,
        &selected,
        &unresolved,
        &semantic_digest,
        &resource.semantic_digest,
        &resource.definition_version,
        &resource.definition_digest,
        &resource.method_requirements,
        &resource.selected,
        &resource.unresolved_needs,
        &resource.freshness_warnings,
    ))?;
    let digest_value = if resource.inquiry.is_some() {
        digest(&(&base_digest, &resource.inquiry, &resource.projection_policy))?
    } else {
        base_digest
    };
    resource.digest = digest_value.clone();
    let value = PipelineKnowledgeManifest {
        id,
        digest: digest_value,
        semantic_digest,
        workspace_generation: generation,
        run_id: run,
        run_revision,
        phase_id: phase.into(),
        selected,
        unresolved_needs: unresolved,
    };
    if serde_json::to_vec(&(&value, &resource))
        .map_err(storage_error)?
        .len()
        > DK_MAX_MANIFEST_BYTES
    {
        return Err(Error::CapacityExceeded);
    }
    sqlx::query("INSERT INTO pipeline_knowledge_manifests(id,tenant_id,workspace_id,run_id,run_revision,phase_id,workspace_generation,digest,semantic_digest,selected,unresolved_needs,contract_version,definition_version,definition_digest,method_requirements,selected_resources,resource_unresolved_needs,freshness_warnings,resource_semantic_digest,resource_inquiry,resource_projection_policy) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'dk-2',$12,$13,$14,$15,$16,$17,$18,$19,$20)")
        .bind(id).bind(tenant).bind(workspace).bind(run).bind(run_revision).bind(phase).bind(generation).bind(&value.digest).bind(&value.semantic_digest).bind(json(&value.selected)?).bind(json(&value.unresolved_needs)?)
        .bind(&resource.definition_version).bind(&resource.definition_digest).bind(json(&resource.method_requirements)?).bind(json(&resource.selected)?).bind(json(&resource.unresolved_needs)?).bind(json(&resource.freshness_warnings)?).bind(&resource.semantic_digest)
        .bind(resource.inquiry.as_ref().map(json).transpose()?)
        .bind(inquiry_projection.as_ref().map(|value| value.policy_name()))
        .execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_lifecycle::erase::register_pipeline_manifest_copies(tx, tenant, workspace, id)
        .await?;
    crate::knowledge_maintenance::register_manifest_consumers(tx, tenant, workspace, id).await?;
    Ok(Some(Captured {
        manifest: value,
        snapshot,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn preview(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    principal: Uuid,
) -> Result<(i64, bool, String, Vec<PipelineKnowledgeItem>, Vec<String>)> {
    let state:Option<(i64,bool)>=sqlx::query_as("SELECT generation,capability_ready FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2").bind(tenant).bind(workspace).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((generation, ready)) = state else {
        return Ok((
            0,
            false,
            digest(&(Vec::<String>::new(), Vec::<String>::new()))?,
            Vec::new(),
            Vec::new(),
        ));
    };
    if !ready {
        return Ok((
            generation,
            false,
            digest(&(Vec::<String>::new(), Vec::<String>::new()))?,
            Vec::new(),
            Vec::new(),
        ));
    }
    delivery::require_identity_ready(tx).await?;
    let inquiry_projection = inquiry::load(tx, tenant, workspace, run).await?;
    let (revisions, gaps) = if inquiry_projection
        .as_ref()
        .and_then(|value| value.stage())
        .is_some()
    {
        (Vec::new(), Vec::new())
    } else {
        projection(tx, tenant, workspace, run, scope, slice, phase, principal).await?
    };
    let selected = items(&revisions);
    Ok((
        generation,
        true,
        legacy::semantic(&selected, &gaps)?,
        selected,
        gaps,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn status(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: Option<&str>,
    manifest: Option<&PipelineKnowledgeManifest>,
) -> Result<Option<PipelineKnowledgeStatus>> {
    let Some(phase) = phase else { return Ok(None) };
    let (generation, ready, current, current_items, gaps) =
        preview(tx, tenant, workspace, run, scope, slice, phase, principal).await?;
    if !ready {
        return Ok(Some(PipelineKnowledgeStatus {
            state: PipelineKnowledgeState::Inactive,
            current_generation: generation,
            changed_unit_ids: Vec::new(),
        }));
    }
    let Some(old) = manifest else {
        return Ok(Some(PipelineKnowledgeStatus {
            state: PipelineKnowledgeState::NeedsContext,
            current_generation: generation,
            changed_unit_ids: current_items.iter().map(|v| v.unit_id).collect(),
        }));
    };
    let old_items: BTreeMap<_, _> = old
        .selected
        .iter()
        .map(|v| (v.unit_id, (v.revision, v.rdf_digest.as_str())))
        .collect();
    let new_items: BTreeMap<_, _> = current_items
        .iter()
        .map(|v| (v.unit_id, (v.revision, v.rdf_digest.as_str())))
        .collect();
    let mut changed: BTreeSet<Uuid> = old_items
        .keys()
        .chain(new_items.keys())
        .filter(|id| old_items.get(id) != new_items.get(id))
        .copied()
        .collect();
    changed.extend(
        gaps.iter()
            .filter_map(|gap| gap.rsplit_once(':').map(|(_, id)| id))
            .filter_map(|id| Uuid::parse_str(id).ok()),
    );
    let state = if !gaps.is_empty() {
        PipelineKnowledgeState::NeedsContext
    } else if old.workspace_generation == generation && old.semantic_digest == current {
        PipelineKnowledgeState::Current
    } else {
        PipelineKnowledgeState::Stale
    };
    Ok(Some(PipelineKnowledgeStatus {
        state,
        current_generation: generation,
        changed_unit_ids: changed.into_iter().collect(),
    }))
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
