use super::*;

type OwnedCopyKey = (String, Uuid, i64, Option<String>, Option<Uuid>);

async fn authorize_copy_keys(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    keys: Vec<OwnedCopyKey>,
) -> Result<()> {
    for (relation, row, revision, operation, request) in keys {
        crate::durable_knowledge::manifest::authorize_owned_copy(
            tx,
            tenant,
            workspace,
            principal,
            &relation,
            row,
            revision,
            operation.as_deref(),
            request,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn authorize_run_origin(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
) -> Result<()> {
    let keys:Vec<OwnedCopyKey>=sqlx::query_as("SELECT relation_name,row_id,row_revision,row_operation,row_request_id FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND relation_name='slice_pipeline_runs' AND row_id=$3 ORDER BY row_revision,row_operation,row_request_id")
        .bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
    if keys.is_empty() {
        return Err(Error::InternalInvariant);
    }
    authorize_copy_keys(tx, tenant, workspace, principal, keys).await
}

pub(crate) async fn authorize_run_origin_if_present(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
) -> Result<()> {
    let keys:Vec<OwnedCopyKey>=sqlx::query_as("SELECT relation_name,row_id,row_revision,row_operation,row_request_id FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND relation_name='slice_pipeline_runs' AND row_id=$3 ORDER BY row_revision,row_operation,row_request_id")
        .bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
    authorize_copy_keys(tx, tenant, workspace, principal, keys).await
}

pub(super) async fn authorize_context_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
) -> Result<()> {
    let keys:Vec<OwnedCopyKey>=sqlx::query_as("SELECT DISTINCT c.relation_name,c.row_id,c.row_revision,c.row_operation,c.row_request_id FROM knowledge_owned_copies c WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND ((c.relation_name='slice_pipeline_runs' AND c.row_id=$3) OR (c.relation_name='slice_pipeline_phase_attempts' AND EXISTS(SELECT 1 FROM slice_pipeline_phase_attempts a WHERE a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.row_id AND a.run_id=$3)) OR (c.relation_name='slice_pipeline_phase_outputs' AND EXISTS(SELECT 1 FROM slice_pipeline_phase_outputs o WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.id=c.row_id AND o.run_id=$3)) OR (c.relation_name='slice_pipeline_inputs' AND EXISTS(SELECT 1 FROM slice_pipeline_inputs i WHERE i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id AND i.id=c.row_id AND i.run_id=$3)) OR (c.relation_name='slice_pipeline_receipts' AND c.row_id=$3) OR (c.relation_name='slice_results' AND EXISTS(SELECT 1 FROM slice_results r WHERE r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.id=c.row_id AND r.pipeline_run_id=$3))) ORDER BY c.relation_name,c.row_id,c.row_revision,c.row_operation,c.row_request_id")
        .bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
    authorize_copy_keys(tx, tenant, workspace, principal, keys).await
}

pub(crate) async fn authorize_frozen_replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
    frozen: &PipelineRunContext,
) -> Result<()> {
    authorize_context_copies(tx, tenant, workspace, principal, run).await?;
    let erased: bool = sqlx::query_scalar("SELECT payload_erased FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(run).fetch_optional(&mut **tx).await.map_err(storage_error)?
        .ok_or(Error::NotFound)?;
    if erased {
        return Err(Error::KnowledgePayloadErased);
    }
    if let Some(header) = frozen.knowledge_resources_paged.as_ref() {
        let (current, _) = crate::durable_knowledge::manifest::load_paged_resources(
            tx,
            tenant,
            workspace,
            Some(header.id),
            principal,
        )
        .await?
        .ok_or(Error::InternalInvariant)?;
        if current != *header || current.run_id != run {
            return Err(Error::InternalInvariant);
        }
    }
    if let Some(manifest) = frozen.knowledge_resources.as_ref() {
        let current = crate::durable_knowledge::manifest::load_resources(
            tx,
            tenant,
            workspace,
            Some(manifest.id),
            principal,
        )
        .await?
        .ok_or(Error::InternalInvariant)?;
        if current != *manifest {
            return Err(Error::InternalInvariant);
        }
    }
    if let Some(manifest) = frozen.knowledge.as_ref() {
        crate::durable_knowledge::manifest::authorize_manifest(
            tx,
            tenant,
            workspace,
            manifest.id,
            principal,
        )
        .await?
        .ok_or(Error::Forbidden)?;
    }
    Ok(())
}
