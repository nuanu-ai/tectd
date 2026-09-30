use super::*;
use std::collections::{BTreeMap, BTreeSet};

mod capture;
mod delivery;
mod generic;
mod inquiry;
mod legacy;
pub(crate) use capture::{capture, capture_with_proofs};
pub(crate) use delivery::{
    authorize_manifest, authorize_owned_copy, load, load_resources, resource_status,
    resource_status_with_proofs,
};

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
    revisions
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
        .collect()
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
pub(crate) async fn validate_completion_with_proofs(
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
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<()> {
    validate_completion_inner(
        tx,
        tenant,
        workspace,
        run,
        scope,
        slice,
        phase,
        session,
        manifest,
        consumed,
        Some(proofs),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn validate_completion_inner(
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
    mut proofs: Option<&mut crate::knowledge_lifecycle::PublicationProofScope>,
) -> Result<()> {
    let principal: Option<Uuid> = sqlx::query_scalar("SELECT tect_dk_session_principal($1)")
        .bind(session)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let principal = principal.ok_or(Error::Forbidden)?;
    if let Some(proofs) = proofs.as_ref() {
        proofs.require_identity(tenant, workspace, principal, session)?;
    }
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
    let resources = load_resources(tx, tenant, workspace, Some(manifest.id), principal).await?;
    let current_resources = match proofs.as_mut() {
        Some(proofs) => {
            generic::snapshot_with_proofs(
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
                session,
                proofs,
            )
            .await?
        }
        None => {
            generic::snapshot(
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
            .await?
        }
    };
    if !gaps.is_empty() {
        return Err(Error::NeedsContext);
    }
    if !current_resources.blocking_gaps.is_empty() {
        return Err(Error::NeedsContext);
    }
    if manifest.workspace_generation != generation || manifest.semantic_digest != current {
        return Err(Error::ContextChanged);
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
        return Ok(match prior {
            RefreshPipelineKnowledgeOutcome::Refreshed(v)
            | RefreshPipelineKnowledgeOutcome::Replay(v) => {
                RefreshPipelineKnowledgeOutcome::Replay(v)
            }
        });
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
        return Ok(match prior {
            RefreshPipelineKnowledgeOutcome::Refreshed(v)
            | RefreshPipelineKnowledgeOutcome::Replay(v) => {
                RefreshPipelineKnowledgeOutcome::Replay(v)
            }
        });
    }
    let row:Option<(Uuid,Uuid,i64,Option<String>)>=sqlx::query_as("SELECT scope_id,slice_id,revision,current_phase_id FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let (scope, slice, revision, current) = row.ok_or(Error::NotFound)?;
    if revision != request.run_revision || current.as_deref() != Some(&request.phase_id) {
        return Err(Error::StaleRevision);
    }
    let next = revision.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query("UPDATE slice_pipeline_runs SET revision=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).bind(next).execute(&mut **tx).await.map_err(storage_error)?;
    let value = capture(
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
    let blocking: serde_json::Value = sqlx::query_scalar("SELECT resource_unresolved_needs FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(value.id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !decode::<Vec<String>>(blocking)?.is_empty() {
        return Err(Error::NeedsContext);
    }
    sqlx::query("UPDATE slice_pipeline_runs SET knowledge_manifest_id=$4,knowledge_manifest_digest=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).bind(value.id).bind(&value.digest).execute(&mut **tx).await.map_err(storage_error)?;
    let outcome = RefreshPipelineKnowledgeOutcome::Refreshed(value);
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
