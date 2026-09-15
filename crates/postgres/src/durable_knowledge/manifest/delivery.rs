use super::*;
use std::collections::BTreeSet;

type GenericManifestRow = (
    String,
    String,
    i64,
    Uuid,
    i64,
    String,
    String,
    String,
    serde_json::Value,
    serde_json::Value,
    serde_json::Value,
    serde_json::Value,
    Option<serde_json::Value>,
    Option<String>,
);

pub(super) async fn require_identity_ready(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let ready: bool = sqlx::query_scalar("SELECT public.tect_dk_database_identity_ready()")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if !ready {
        return Err(Error::KnowledgeUnavailable);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn authorize_owned_copy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    relation: &str,
    row: Uuid,
    row_revision: i64,
    row_operation: Option<&str>,
    row_request_id: Option<Uuid>,
) -> Result<()> {
    require_identity_ready(tx).await?;
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let (copies, missing_head, restricted): (i64, bool, bool) = sqlx::query_as(
        "SELECT count(*),COALESCE(pg_catalog.bool_or(h.unit_id IS NULL),false),COALESCE(pg_catalog.bool_or(h.access_scope='owners_only'),false) FROM knowledge_owned_copies c LEFT JOIN knowledge_unit_heads h ON h.tenant_id=c.tenant_id AND h.workspace_id=c.workspace_id AND h.unit_id=c.unit_id WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.relation_name=$3 AND c.row_id=$4 AND c.row_revision=$5 AND c.row_operation IS NOT DISTINCT FROM $6 AND c.row_request_id IS NOT DISTINCT FROM $7",
    )
    .bind(tenant).bind(workspace).bind(relation).bind(row).bind(row_revision)
    .bind(row_operation).bind(row_request_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if copies == 0 || missing_head {
        return Err(Error::InternalInvariant);
    }
    if restricted && !owner {
        return Err(Error::Forbidden);
    }
    Ok(())
}

pub(crate) async fn authorize_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    id: Uuid,
    principal: Uuid,
) -> Result<Option<String>> {
    require_identity_ready(tx).await?;
    let marker: Option<(bool, String)> = sqlx::query_as("SELECT payload_erased,contract_version FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((erased, contract)) = marker else {
        return Ok(None);
    };
    if erased {
        let keys: Vec<(i64, Option<String>, Option<Uuid>)> = sqlx::query_as(
            "SELECT DISTINCT row_revision,row_operation,row_request_id FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND relation_name='pipeline_knowledge_manifests' AND row_id=$3 ORDER BY row_revision,row_operation,row_request_id",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(id)
        .fetch_all(&mut **tx)
        .await
        .map_err(storage_error)?;
        if keys.is_empty() {
            return Err(Error::InternalInvariant);
        }
        for (revision, operation, request) in keys {
            authorize_owned_copy(
                tx,
                tenant,
                workspace,
                principal,
                "pipeline_knowledge_manifests",
                id,
                revision,
                operation.as_deref(),
                request,
            )
            .await?;
        }
        return Err(Error::KnowledgePayloadErased);
    }
    let missing_or_erased: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pipeline_knowledge_manifests m CROSS JOIN LATERAL (SELECT value AS resource FROM pg_catalog.jsonb_array_elements(m.selected) UNION ALL SELECT value FROM pg_catalog.jsonb_array_elements(COALESCE(m.selected_resources,'[]'::jsonb))) selected WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 AND NOT EXISTS(SELECT 1 FROM knowledge_unit_heads h JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=(selected.resource->>'revision')::bigint WHERE h.tenant_id=m.tenant_id AND h.workspace_id=m.workspace_id AND h.unit_id=(selected.resource->>'unit_id')::uuid AND NOT h.payload_erased AND NOT r.payload_erased))")
        .bind(tenant).bind(workspace).bind(id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if missing_or_erased {
        return Err(Error::KnowledgePayloadErased);
    }
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if !owner {
        let inaccessible: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pipeline_knowledge_manifests m CROSS JOIN LATERAL (SELECT value AS resource FROM pg_catalog.jsonb_array_elements(m.selected) UNION ALL SELECT value FROM pg_catalog.jsonb_array_elements(COALESCE(m.selected_resources,'[]'::jsonb))) selected JOIN knowledge_unit_heads h ON h.tenant_id=m.tenant_id AND h.workspace_id=m.workspace_id AND h.unit_id=(selected.resource->>'unit_id')::uuid JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=(selected.resource->>'revision')::bigint WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 AND (h.access_scope='owners_only' OR r.access_scope='owners_only'))")
            .bind(tenant).bind(workspace).bind(id).fetch_one(&mut **tx).await.map_err(storage_error)?;
        if inaccessible {
            return Err(Error::Forbidden);
        }
    }
    Ok(Some(contract))
}

#[allow(clippy::type_complexity)]
pub(crate) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    id: Option<Uuid>,
    principal: Uuid,
) -> Result<Option<PipelineKnowledgeManifest>> {
    let Some(id) = id else { return Ok(None) };
    if authorize_manifest(tx, tenant, workspace, id, principal)
        .await?
        .is_none()
    {
        return Ok(None);
    }
    let row:Option<(String,String,i64,Uuid,i64,String,serde_json::Value,serde_json::Value)>=sqlx::query_as("SELECT digest,semantic_digest,workspace_generation,run_id,run_revision,phase_id,selected,unresolved_needs FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT payload_erased")
        .bind(tenant).bind(workspace).bind(id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    row.map(|v| {
        Ok(PipelineKnowledgeManifest {
            id,
            digest: v.0,
            semantic_digest: v.1,
            workspace_generation: v.2,
            run_id: v.3,
            run_revision: v.4,
            phase_id: v.5,
            selected: decode(v.6)?,
            unresolved_needs: decode(v.7)?,
        })
    })
    .transpose()
}

pub(crate) async fn load_resources(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    id: Option<Uuid>,
    principal: Uuid,
) -> Result<Option<PipelineKnowledgeResourceManifest>> {
    let Some(id) = id else { return Ok(None) };
    let Some(contract) = authorize_manifest(tx, tenant, workspace, id, principal).await? else {
        return Ok(None);
    };
    if contract == "dk-1" {
        return Ok(None);
    }
    if contract != "dk-2" {
        return Err(Error::InternalInvariant);
    }
    let row: Option<GenericManifestRow> = sqlx::query_as("SELECT digest,resource_semantic_digest,workspace_generation,run_id,run_revision,phase_id,definition_version,definition_digest,method_requirements,selected_resources,resource_unresolved_needs,freshness_warnings,resource_inquiry,resource_projection_policy FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND contract_version='dk-2' AND NOT payload_erased")
        .bind(tenant).bind(workspace).bind(id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    row.map(|v| {
        let inquiry = v.12.map(decode).transpose()?;
        let projection_policy =
            v.13.map(|value| decode(serde_json::Value::String(value)))
                .transpose()?;
        if inquiry.is_some() != projection_policy.is_some() {
            return Err(Error::InternalInvariant);
        }
        Ok(PipelineKnowledgeResourceManifest {
            id,
            digest: v.0,
            semantic_digest: v.1,
            workspace_generation: v.2,
            run_id: v.3,
            run_revision: v.4,
            phase_id: v.5,
            definition_version: v.6,
            definition_digest: v.7,
            method_requirements: decode(v.8)?,
            inquiry,
            projection_policy,
            selected: decode(v.9)?,
            unresolved_needs: decode(v.10)?,
            freshness_warnings: decode(v.11)?,
        })
    })
    .transpose()
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn resource_status(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: Option<&str>,
    manifest: Option<&PipelineKnowledgeResourceManifest>,
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
    require_identity_ready(tx).await?;
    let current = super::generic::snapshot(
        tx,
        tenant,
        workspace,
        principal,
        run,
        run_revision,
        scope,
        slice,
        phase,
        manifest.map_or_else(Uuid::nil, |v| v.id),
        manifest.map_or_else(String::new, |v| v.digest.clone()),
    )
    .await?;
    let Some(old) = manifest else {
        return Ok(Some(PipelineKnowledgeResourceStatus {
            state: PipelineKnowledgeResourceState::NeedsContext,
            current_generation: generation,
            changed_unit_ids: current
                .manifest
                .selected
                .iter()
                .map(|v| v.unit_id)
                .collect(),
            freshness_warnings: current.manifest.freshness_warnings,
            access_changed: current
                .blocking_gaps
                .iter()
                .any(|v| v == "resource_inaccessible"),
        }));
    };
    let old_items: BTreeSet<_> = old
        .selected
        .iter()
        .map(|v| {
            (
                v.unit_id,
                v.revision,
                v.rdf_digest.clone(),
                v.binding.binding_iri.clone(),
            )
        })
        .collect();
    let new_items: BTreeSet<_> = current
        .manifest
        .selected
        .iter()
        .map(|v| {
            (
                v.unit_id,
                v.revision,
                v.rdf_digest.clone(),
                v.binding.binding_iri.clone(),
            )
        })
        .collect();
    let changed_unit_ids = old_items
        .symmetric_difference(&new_items)
        .map(|v| v.0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let same_basis = old.workspace_generation == generation
        && old.run_revision == run_revision
        && old.semantic_digest == current.manifest.semantic_digest
        && old.definition_version == current.manifest.definition_version
        && old.definition_digest == current.manifest.definition_digest
        && old.method_requirements == current.manifest.method_requirements
        && old.inquiry == current.manifest.inquiry
        && old.projection_policy == current.manifest.projection_policy;
    let state = if !current.blocking_gaps.is_empty() {
        PipelineKnowledgeResourceState::NeedsContext
    } else if same_basis {
        PipelineKnowledgeResourceState::Current
    } else {
        PipelineKnowledgeResourceState::Stale
    };
    Ok(Some(PipelineKnowledgeResourceStatus {
        state,
        current_generation: generation,
        changed_unit_ids,
        freshness_warnings: current.manifest.freshness_warnings,
        access_changed: current
            .blocking_gaps
            .iter()
            .any(|v| v == "resource_inaccessible"),
    }))
}
