use super::*;

pub(crate) async fn resolve(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &ResolvePipelineCheckpoint,
) -> Result<ResolvePipelineCheckpointOutcome> {
    let payload = json(request)?;
    if let Some(value) = replay(tx, tenant, workspace, session, request, &payload).await? {
        return Ok(value);
    }
    let _ = crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let principal = session_principal(tx, session).await?;
    checkpoint::authorize(
        tx,
        tenant,
        workspace,
        principal,
        request.checkpoint.checkpoint_id,
    )
    .await?;
    if request.action == ResolvePipelineCheckpointAction::Accept {
        checkpoint::validate_open_basis(tx, tenant, workspace, &request.checkpoint).await?;
    }
    let run: (i64, String, Option<String>, String, bool) = sqlx::query_as(
        "SELECT revision,status,current_phase_id,definition_kind,payload_erased FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant).bind(workspace).bind(request.producer_run_id)
    .fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    if run.4 {
        return Err(Error::KnowledgePayloadErased);
    }
    if run.0 != request.producer_run_revision {
        return Err(Error::StaleRevision);
    }
    let phase_id = run.2.as_deref().ok_or(Error::InternalInvariant)?;
    let phase_ordinal: Option<i32> = sqlx::query_scalar(
        "SELECT current_phase_ordinal FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant).bind(workspace).bind(request.producer_run_id)
    .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if run.1 != "waiting_input"
        || phase_ordinal != Some(5)
        || run.3 != PipelineKind::DeepBrainstorming.as_str()
    {
        return Err(Error::Forbidden);
    }
    let row: (String, Uuid, Option<Uuid>, bool) = sqlx::query_as(
        "SELECT status,producer_run_id,consumer_run_id,payload_erased FROM pipeline_research_checkpoints WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND digest=$4 FOR UPDATE",
    )
    .bind(tenant).bind(workspace).bind(request.checkpoint.checkpoint_id).bind(&request.checkpoint.digest)
    .fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    if row.3 {
        return Err(Error::KnowledgePayloadErased);
    }
    if row.0 != "open" || row.1 != request.producer_run_id {
        return Err(Error::Forbidden);
    }
    let consumer_result = match request.action {
        ResolvePipelineCheckpointAction::Accept | ResolvePipelineCheckpointAction::Reject => {
            let consumer = row.2.ok_or(Error::InputPending)?;
            let terminal = request.terminal.as_ref().ok_or(Error::InvalidArguments)?;
            validate_terminal(
                tx,
                tenant,
                workspace,
                principal,
                request.checkpoint.checkpoint_id,
                consumer,
                terminal,
            )
            .await?;
            Some(terminal.result_id)
        }
        ResolvePipelineCheckpointAction::Cancel => None,
    };
    let state = match request.action {
        ResolvePipelineCheckpointAction::Accept => "accepted",
        ResolvePipelineCheckpointAction::Reject => "rejected",
        ResolvePipelineCheckpointAction::Cancel => "cancelled",
    };
    let action = enum_text(&request.action)?;
    let terminal = request.terminal.as_ref();
    sqlx::query(
        "UPDATE pipeline_research_checkpoints SET status=$5,consumer_result_id=$6,consumer_terminal_output_id=$7,consumer_terminal_output_digest=$8,resolution_action=$9,resolution_reason=$10,resolved_by_session_id=$11,resolved_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND digest=$4 AND status='open'",
    )
    .bind(tenant).bind(workspace).bind(request.checkpoint.checkpoint_id).bind(&request.checkpoint.digest)
    .bind(state).bind(consumer_result).bind(terminal.map(|value| value.output_id))
    .bind(terminal.map(|value| value.output_digest.as_str())).bind(&action).bind(&request.reason).bind(session)
    .execute(&mut **tx).await.map_err(storage_error)?;
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence),0)+1 FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3",
    )
    .bind(tenant).bind(workspace).bind(request.producer_run_id)
    .fetch_one(&mut **tx).await.map_err(storage_error)?;
    let input_id = Uuid::new_v4();
    let input = format!(
        "Research checkpoint {} {}: {}",
        request.checkpoint.checkpoint_id, action, request.reason
    );
    let input_digest = digest(&serde_json::json!({
        "checkpoint": request.checkpoint,
        "action": request.action,
        "reason": request.reason,
        "terminal": request.terminal,
    }))?;
    sqlx::query(
        "INSERT INTO slice_pipeline_inputs(id,tenant_id,workspace_id,run_id,sequence,phase_id,input,input_digest,actor_session_id,request_id,request_payload,checkpoint_id,checkpoint_digest,checkpoint_result_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
    )
    .bind(input_id).bind(tenant).bind(workspace).bind(request.producer_run_id).bind(sequence)
    .bind(phase_id).bind(input).bind(input_digest).bind(session).bind(request.request_id).bind(&payload)
    .bind(request.checkpoint.checkpoint_id).bind(&request.checkpoint.digest).bind(consumer_result)
    .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_pipeline_runs SET revision=revision+1,status='active' WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.producer_run_id)
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO pipeline_checkpoint_receipts(tenant_id,workspace_id,checkpoint_id,request_id,actor_session_id,request_payload,result_payload) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(tenant).bind(workspace).bind(request.checkpoint.checkpoint_id).bind(request.request_id)
        .bind(session).bind(payload).bind(serde_json::json!({}))
        .execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_lifecycle::erase::register_checkpoint_resolution_copies(
        tx,
        tenant,
        workspace,
        request.checkpoint.checkpoint_id,
        input_id,
        request.request_id,
    )
    .await?;
    let context = load_context(tx, tenant, workspace, principal, request.producer_run_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let checkpoint = context
        .checkpoints
        .iter()
        .find(|value| value.checkpoint == request.checkpoint)
        .cloned()
        .ok_or(Error::InternalInvariant)?;
    let outcome = ResolvePipelineCheckpointOutcome {
        checkpoint,
        context,
    };
    let encoded = json(&outcome)?;
    sqlx::query("UPDATE slice_pipeline_inputs SET result_payload=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(input_id).bind(&encoded)
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE pipeline_checkpoint_receipts SET result_payload=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).bind(encoded)
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(outcome)
}

async fn validate_terminal(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    checkpoint: Uuid,
    consumer: Uuid,
    terminal: &PipelineCheckpointTerminalRef,
) -> Result<()> {
    load_context(tx, tenant, workspace, principal, consumer)
        .await?
        .ok_or(Error::NotFound)?;
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM slice_pipeline_runs r JOIN slice_results sr ON sr.tenant_id=r.tenant_id AND sr.workspace_id=r.workspace_id AND sr.pipeline_run_id=r.id JOIN slice_pipeline_output_bindings b ON b.tenant_id=r.tenant_id AND b.workspace_id=r.workspace_id AND b.run_id=r.id JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.id=$3 AND r.definition_kind='slice.research' AND r.status='completed' AND r.source_checkpoint_id=$4 AND sr.id=$5 AND sr.outcome='completed' AND NOT sr.payload_erased AND o.id=$6 AND o.body_digest=$7 AND NOT o.payload_erased AND NOT b.stale AND b.phase_ordinal=(SELECT MAX(phase_ordinal) FROM slice_pipeline_output_bindings WHERE tenant_id=r.tenant_id AND workspace_id=r.workspace_id AND run_id=r.id))",
    )
    .bind(tenant).bind(workspace).bind(consumer).bind(checkpoint).bind(terminal.result_id)
    .bind(terminal.output_id).bind(&terminal.output_digest)
    .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if valid { Ok(()) } else { Err(Error::Forbidden) }
}

async fn replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &ResolvePipelineCheckpoint,
    payload: &serde_json::Value,
) -> Result<Option<ResolvePipelineCheckpointOutcome>> {
    let row: Option<(Option<serde_json::Value>, Option<serde_json::Value>, bool)> = sqlx::query_as(
        "SELECT request_payload,result_payload,payload_erased FROM pipeline_checkpoint_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    )
    .bind(tenant).bind(workspace).bind(request.request_id)
    .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((stored, result, erased)) = row else {
        return Ok(None);
    };
    let principal = session_principal(tx, session).await?;
    checkpoint::authorize(
        tx,
        tenant,
        workspace,
        principal,
        request.checkpoint.checkpoint_id,
    )
    .await?;
    if erased {
        return Err(Error::KnowledgePayloadErased);
    }
    if stored.as_ref() != Some(payload) {
        return Err(Error::InputConflict);
    }
    load_context(tx, tenant, workspace, principal, request.producer_run_id)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Some(decode(result.ok_or(Error::InternalInvariant)?)?))
}
