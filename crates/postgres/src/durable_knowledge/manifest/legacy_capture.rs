use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[allow(clippy::too_many_arguments)]
pub(super) async fn projection(
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

pub(super) fn items(revisions: &[KnowledgeUnitRevision]) -> Vec<PipelineKnowledgeItem> {
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

pub(super) fn unique_legacy_items(
    selected: Vec<PipelineKnowledgeItem>,
) -> Vec<PipelineKnowledgeItem> {
    let mut seen = BTreeSet::new();
    selected
        .into_iter()
        .filter(|item| seen.insert((item.unit_id, item.revision, item.rdf_digest.clone())))
        .collect()
}

pub(crate) struct Captured {
    pub manifest: PipelineKnowledgeManifest,
    pub(super) snapshot: generic::Snapshot,
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
pub(super) async fn preview(
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
