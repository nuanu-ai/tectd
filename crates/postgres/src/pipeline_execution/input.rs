use super::*;

pub(crate) async fn record_input(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &RecordPipelineInput,
) -> Result<PipelineMutationOutcome> {
    let payload = json(request)?;
    if let Some((stored,result,erased))=sqlx::query_as::<_,(Option<serde_json::Value>,Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased { return Err(Error::KnowledgePayloadErased) }
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    let row:(i64,String,Option<String>)=sqlx::query_as("SELECT revision,status,current_phase_id FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    if let Some((stored,result,erased))=sqlx::query_as::<_,(Option<serde_json::Value>,Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased { return Err(Error::KnowledgePayloadErased) }
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    if row.0 != request.run_revision {
        return Err(Error::StaleRevision);
    }
    if row.2.as_deref() != Some(&request.phase_id)
        || matches!(row.1.as_str(), "completed" | "escalated")
    {
        return Err(Error::StaleContext);
    }
    let sequence:i64=sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0)+1 FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let id = Uuid::new_v4();
    let input_digest = digest(&request.input)?;
    sqlx::query("INSERT INTO slice_pipeline_inputs(id,tenant_id,workspace_id,run_id,sequence,phase_id,input,input_digest,actor_session_id,request_id,request_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(id).bind(tenant).bind(workspace).bind(request.run_id).bind(sequence).bind(&request.phase_id).bind(&request.input).bind(input_digest).bind(session).bind(request.request_id).bind(&payload)
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_pipeline_runs SET revision=revision+1,status='active' WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).execute(&mut **tx).await.map_err(storage_error)?;
    let principal = session_principal(tx, session).await?;
    let outcome = PipelineMutationOutcome {
        context: load_context(tx, tenant, workspace, principal, request.run_id)
            .await?
            .ok_or(Error::InternalInvariant)?,
        result: None,
    };
    sqlx::query("UPDATE slice_pipeline_inputs SET result_payload=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(id).bind(json(&outcome)?).execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_lifecycle::erase::register_pipeline_input_copies(
        tx,
        tenant,
        workspace,
        request.run_id,
        id,
        request.request_id,
    )
    .await?;
    Ok(outcome)
}

pub(crate) async fn escalate_delivery(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &EscalatePipelineDelivery,
) -> Result<PipelineMutationOutcome> {
    let payload = json(request)?;
    if let Some((stored,result,erased))=sqlx::query_as::<_,(Option<serde_json::Value>,Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND operation='delivery_escalate' AND request_id=$4")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased { return Err(Error::KnowledgePayloadErased) }
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    let row:(i64,String,String,Option<String>)=sqlx::query_as("SELECT revision,status,delivery_mode,current_phase_id FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    if let Some((stored,result,erased))=sqlx::query_as::<_,(Option<serde_json::Value>,Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND operation='delivery_escalate' AND request_id=$4")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased { return Err(Error::KnowledgePayloadErased) }
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    if row.0 != request.run_revision {
        return Err(Error::StaleRevision);
    }
    if row.1 == "completed"
        || row.1 == "escalated"
        || row.2 != "whole"
        || row.3.as_deref() != Some(&request.phase_id)
    {
        return Err(Error::Forbidden);
    }
    sqlx::query("UPDATE slice_pipeline_runs SET revision=revision+1,delivery_mode='phasewise' WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).execute(&mut **tx).await.map_err(storage_error)?;
    let principal = session_principal(tx, session).await?;
    let outcome = PipelineMutationOutcome {
        context: load_context(tx, tenant, workspace, principal, request.run_id)
            .await?
            .ok_or(Error::InternalInvariant)?,
        result: None,
    };
    sqlx::query("INSERT INTO slice_pipeline_receipts(tenant_id,workspace_id,run_id,operation,request_id,actor_session_id,request_payload,result_payload) VALUES($1,$2,$3,'delivery_escalate',$4,$5,$6,$7)")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(request.request_id).bind(session).bind(payload).bind(json(&outcome)?)
        .execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_lifecycle::erase::register_pipeline_receipt_copies(
        tx,
        tenant,
        workspace,
        request.run_id,
        request.request_id,
    )
    .await?;
    Ok(outcome)
}
