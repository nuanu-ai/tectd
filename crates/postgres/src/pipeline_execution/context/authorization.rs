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

pub(in crate::pipeline_execution) async fn authorize_run_origin(
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

pub(in crate::pipeline_execution) async fn authorize_run_origin_if_present(
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
