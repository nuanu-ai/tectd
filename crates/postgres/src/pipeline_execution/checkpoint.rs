use super::*;
use sqlx::Row;

type OwnedCopyKey = (String, Uuid, i64, Option<String>, Option<Uuid>);

#[allow(clippy::too_many_arguments)]
pub(super) async fn create(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &CompletePipelinePhase,
    run: &super::phase::LockedRun,
    attempt_id: Uuid,
    output_id: Uuid,
    output_revision: i64,
    output_digest: &str,
    next_run_revision: i64,
) -> Result<Option<PipelineCheckpointRef>> {
    let Some(draft) = request.research_checkpoint.as_ref() else {
        return Ok(None);
    };
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pipeline_research_checkpoints WHERE tenant_id=$1 AND workspace_id=$2 AND producer_run_id=$3 AND status='open')",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?
    {
        return Err(Error::InputPending);
    }
    let checkpoint_id = Uuid::new_v4();
    let basis = PipelineCheckpointBasis {
        consumed_outputs: request.consumed_outputs.clone(),
        consumed_inputs: request.consumed_inputs.clone(),
        consumed_knowledge: request.consumed_knowledge.clone(),
    };
    let digest = digest(&serde_json::json!({
        "checkpoint_id": checkpoint_id,
        "producer_run_id": request.run_id,
        "producer_run_revision": next_run_revision,
        "producer_definition_version": run.5,
        "producer_definition_digest": run.6,
        "producer_phase_id": request.phase_id,
        "producer_output_id": output_id,
        "producer_output_revision": output_revision,
        "producer_output_digest": output_digest,
        "basis": basis,
        "question": draft.question,
        "answer_criteria": draft.answer_criteria,
        "inquiry": draft.inquiry,
        "reason": draft.reason,
    }))?;
    sqlx::query(
        "INSERT INTO pipeline_research_checkpoints(id,tenant_id,workspace_id,digest,producer_run_id,producer_run_revision,producer_definition_version,producer_definition_digest,producer_phase_id,producer_attempt_id,producer_output_id,producer_output_revision,producer_output_digest,basis,question,answer_criteria,inquiry,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
    )
    .bind(checkpoint_id)
    .bind(tenant)
    .bind(workspace)
    .bind(&digest)
    .bind(request.run_id)
    .bind(next_run_revision)
    .bind(&run.5)
    .bind(&run.6)
    .bind(&request.phase_id)
    .bind(attempt_id)
    .bind(output_id)
    .bind(output_revision)
    .bind(output_digest)
    .bind(json(&basis)?)
    .bind(&draft.question)
    .bind(&draft.answer_criteria)
    .bind(json(&draft.inquiry)?)
    .bind(&draft.reason)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(Some(PipelineCheckpointRef {
        checkpoint_id,
        digest,
    }))
}

pub(super) async fn mark_superseded_after_rework(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    target_ordinal: u32,
    session: Uuid,
) -> Result<()> {
    sqlx::query(
        "UPDATE pipeline_research_checkpoints SET status='superseded',resolution_action='cancel',resolution_reason='producer basis reworked',resolved_by_session_id=$5,resolved_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND producer_run_id=$3 AND status='open' AND $4<=4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .bind(target_ordinal as i32)
    .bind(session)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(crate) async fn validate_candidate_source(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
    source: &PipelineCheckpointRef,
) -> Result<(PipelineInquiryContract, Uuid)> {
    source.validate()?;
    let row: Option<(serde_json::Value, Uuid, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT c.inquiry,s.candidate_id,c.status,c.consumer_run_id FROM pipeline_research_checkpoints c JOIN slice_pipeline_runs r ON r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.id=c.producer_run_id JOIN native_slices s ON s.tenant_id=r.tenant_id AND s.workspace_id=r.workspace_id AND s.id=r.slice_id WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 AND c.digest=$4 AND r.scope_id=$5 AND NOT c.payload_erased AND NOT r.payload_erased FOR UPDATE OF c",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(source.checkpoint_id)
    .bind(&source.digest)
    .bind(scope)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let (inquiry, producer_candidate, status, consumer) = row.ok_or(Error::NotFound)?;
    if status != "open" || consumer.is_some() {
        return Err(Error::Forbidden);
    }
    validate_open_basis(tx, tenant, workspace, source).await?;
    Ok((decode(inquiry)?, producer_candidate))
}

pub(crate) async fn validate_candidate_lineage(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
    candidate: Uuid,
    source: &PipelineCheckpointRef,
) -> Result<(PipelineInquiryContract, Uuid)> {
    source.validate()?;
    let row: Option<(serde_json::Value, Uuid, String, Option<Uuid>)> = sqlx::query_as(
        "SELECT c.inquiry,s.candidate_id,c.status,c.consumer_run_id FROM pipeline_research_checkpoints c JOIN slice_pipeline_runs r ON r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.id=c.producer_run_id JOIN native_slices s ON s.tenant_id=r.tenant_id AND s.workspace_id=r.workspace_id AND s.id=r.slice_id WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 AND c.digest=$4 AND r.scope_id=$5 AND NOT c.payload_erased AND NOT r.payload_erased FOR UPDATE OF c",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(source.checkpoint_id)
    .bind(&source.digest)
    .bind(scope)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let (inquiry, producer_candidate, status, consumer) = row.ok_or(Error::NotFound)?;
    if let Some(consumer) = consumer {
        let preserved: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM native_slices s JOIN slice_pipeline_runs r ON r.tenant_id=s.tenant_id AND r.workspace_id=s.workspace_id AND r.slice_id=s.id WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.scope_id=$3 AND s.candidate_id=$4 AND r.id=$5 AND r.source_checkpoint_id=$6 AND r.source_checkpoint_digest=$7 AND NOT r.payload_erased)",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(scope)
        .bind(candidate)
        .bind(consumer)
        .bind(source.checkpoint_id)
        .bind(&source.digest)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
        if !preserved {
            return Err(Error::Forbidden);
        }
    } else {
        if status != "open" {
            return Err(Error::Forbidden);
        }
        validate_open_basis(tx, tenant, workspace, source).await?;
    }
    Ok((decode(inquiry)?, producer_candidate))
}

pub(crate) async fn validate_open_basis(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    source: &PipelineCheckpointRef,
) -> Result<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pipeline_research_checkpoints c JOIN slice_pipeline_runs r ON r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.id=c.producer_run_id JOIN slice_pipeline_output_bindings b ON b.tenant_id=c.tenant_id AND b.workspace_id=c.workspace_id AND b.run_id=c.producer_run_id AND b.output_id=c.producer_output_id AND b.output_revision=c.producer_output_revision JOIN slice_pipeline_phase_outputs produced ON produced.tenant_id=b.tenant_id AND produced.workspace_id=b.workspace_id AND produced.id=b.output_id AND produced.body_digest=c.producer_output_digest WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 AND c.digest=$4 AND c.status='open' AND NOT c.payload_erased AND r.status='waiting_input' AND r.current_phase_ordinal=5 AND NOT r.payload_erased AND b.phase_ordinal=5 AND NOT b.stale AND NOT produced.payload_erased AND NOT EXISTS(SELECT 1 FROM pg_catalog.jsonb_array_elements(COALESCE(c.basis->'consumed_outputs','[]'::jsonb)) x LEFT JOIN slice_pipeline_output_bindings current ON current.tenant_id=c.tenant_id AND current.workspace_id=c.workspace_id AND current.run_id=c.producer_run_id AND current.phase_id=x->>'phase_id' AND current.output_revision=(x->>'output_revision')::bigint AND NOT current.stale LEFT JOIN slice_pipeline_phase_outputs output ON output.tenant_id=current.tenant_id AND output.workspace_id=current.workspace_id AND output.id=current.output_id AND output.body_digest=x->>'digest' AND NOT output.payload_erased WHERE output.id IS NULL) AND NOT EXISTS(SELECT 1 FROM pg_catalog.jsonb_array_elements(COALESCE(c.basis->'consumed_inputs','[]'::jsonb)) x LEFT JOIN slice_pipeline_inputs i ON i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id AND i.run_id=c.producer_run_id AND i.id=(x->>'input_id')::uuid AND i.sequence=(x->>'sequence')::bigint AND i.input_digest=x->>'digest' AND NOT i.payload_erased WHERE i.id IS NULL) AND (NOT (c.basis ? 'consumed_knowledge') OR c.basis->'consumed_knowledge' IS NULL OR EXISTS(SELECT 1 FROM pipeline_knowledge_manifests m JOIN workspace_knowledge_state state ON state.tenant_id=m.tenant_id AND state.workspace_id=m.workspace_id WHERE m.tenant_id=c.tenant_id AND m.workspace_id=c.workspace_id AND m.id=(c.basis->'consumed_knowledge'->>'manifest_id')::uuid AND m.digest=c.basis->'consumed_knowledge'->>'digest' AND m.workspace_generation=state.generation AND NOT m.payload_erased)))",
    )
    .bind(tenant).bind(workspace).bind(source.checkpoint_id).bind(&source.digest)
    .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if valid {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

pub(crate) async fn authorize_many(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    checkpoints: &[PipelineResearchCheckpoint],
) -> Result<()> {
    for checkpoint in checkpoints {
        authorize(
            tx,
            tenant,
            workspace,
            principal,
            checkpoint.checkpoint.checkpoint_id,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn authorize(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    checkpoint: Uuid,
) -> Result<()> {
    let keys: Vec<OwnedCopyKey> = sqlx::query_as(
        "SELECT relation_name,row_id,row_revision,row_operation,row_request_id FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND ((relation_name='pipeline_research_checkpoints' AND row_id=$3) OR (relation_name='pipeline_checkpoint_receipts' AND row_id=$3)) ORDER BY relation_name,row_revision,row_operation,row_request_id",
    )
    .bind(tenant).bind(workspace).bind(checkpoint)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
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

pub(super) async fn bind_consumer(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run_id: Uuid,
    scope_id: Uuid,
    inquiry: Option<&PipelineInquiryContract>,
    source: Option<&PipelineCheckpointRef>,
) -> Result<()> {
    let Some(source) = source else { return Ok(()) };
    let (expected, _) = validate_candidate_source(tx, tenant, workspace, scope_id, source).await?;
    if inquiry != Some(&expected) {
        return Err(Error::StaleContext);
    }
    let changed = sqlx::query(
        "UPDATE pipeline_research_checkpoints SET consumer_run_id=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND digest=$4 AND status='open' AND consumer_run_id IS NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(source.checkpoint_id)
    .bind(&source.digest)
    .bind(run_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if changed.rows_affected() == 1 {
        crate::knowledge_lifecycle::erase::register_checkpoint_consumer_copies(
            tx,
            tenant,
            workspace,
            source.checkpoint_id,
            run_id,
        )
        .await?;
        Ok(())
    } else {
        Err(Error::Forbidden)
    }
}

pub(super) async fn ensure_run_source_open(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run_id: Uuid,
) -> Result<()> {
    let source: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT source_checkpoint_id,source_checkpoint_digest FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND source_checkpoint_id IS NOT NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((id, digest)) = source else {
        return Ok(());
    };
    let open: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pipeline_research_checkpoints WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND digest=$4 AND status='open' AND consumer_run_id=$5 AND NOT payload_erased)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(id)
    .bind(digest)
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if open {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

pub(super) async fn load_for_run(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run_id: Uuid,
) -> Result<Vec<PipelineResearchCheckpoint>> {
    let rows = sqlx::query(
        "SELECT id,digest,status,producer_run_id,producer_run_revision,producer_definition_version,producer_definition_digest,producer_phase_id,producer_output_id,producer_output_revision,producer_output_digest,basis,question,answer_criteria,inquiry,reason,consumer_run_id,consumer_result_id,consumer_terminal_output_id,consumer_terminal_output_digest,resolution_action,resolution_reason FROM pipeline_research_checkpoints WHERE tenant_id=$1 AND workspace_id=$2 AND (producer_run_id=$3 OR consumer_run_id=$3) AND NOT payload_erased ORDER BY created_at,id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    rows.into_iter().map(decode_row).collect()
}

pub(crate) async fn load_for_scope(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope_id: Uuid,
) -> Result<Vec<PipelineResearchCheckpoint>> {
    let rows = sqlx::query(
        "SELECT c.id,c.digest,c.status,c.producer_run_id,c.producer_run_revision,c.producer_definition_version,c.producer_definition_digest,c.producer_phase_id,c.producer_output_id,c.producer_output_revision,c.producer_output_digest,c.basis,c.question,c.answer_criteria,c.inquiry,c.reason,c.consumer_run_id,c.consumer_result_id,c.consumer_terminal_output_id,c.consumer_terminal_output_digest,c.resolution_action,c.resolution_reason FROM pipeline_research_checkpoints c JOIN slice_pipeline_runs r ON r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.id=c.producer_run_id WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND r.scope_id=$3 AND NOT c.payload_erased AND NOT r.payload_erased ORDER BY c.created_at,c.id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    rows.into_iter().map(decode_row).collect()
}

fn decode_row(row: sqlx::postgres::PgRow) -> Result<PipelineResearchCheckpoint> {
    Ok(PipelineResearchCheckpoint {
        checkpoint: PipelineCheckpointRef {
            checkpoint_id: row.try_get("id").map_err(storage_error)?,
            digest: row.try_get("digest").map_err(storage_error)?,
        },
        status: decode(serde_json::Value::String(
            row.try_get("status").map_err(storage_error)?,
        ))?,
        producer_run_id: row.try_get("producer_run_id").map_err(storage_error)?,
        producer_run_revision: row
            .try_get("producer_run_revision")
            .map_err(storage_error)?,
        producer_definition_version: row
            .try_get("producer_definition_version")
            .map_err(storage_error)?,
        producer_definition_digest: row
            .try_get("producer_definition_digest")
            .map_err(storage_error)?,
        producer_phase_id: row.try_get("producer_phase_id").map_err(storage_error)?,
        producer_output_id: row.try_get("producer_output_id").map_err(storage_error)?,
        producer_output_revision: row
            .try_get("producer_output_revision")
            .map_err(storage_error)?,
        producer_output_digest: row
            .try_get("producer_output_digest")
            .map_err(storage_error)?,
        basis: decode(row.try_get("basis").map_err(storage_error)?)?,
        question: row.try_get("question").map_err(storage_error)?,
        answer_criteria: row.try_get("answer_criteria").map_err(storage_error)?,
        inquiry: decode(row.try_get("inquiry").map_err(storage_error)?)?,
        reason: row.try_get("reason").map_err(storage_error)?,
        consumer_run_id: row.try_get("consumer_run_id").map_err(storage_error)?,
        consumer_result_id: row.try_get("consumer_result_id").map_err(storage_error)?,
        consumer_terminal_output_id: row
            .try_get("consumer_terminal_output_id")
            .map_err(storage_error)?,
        consumer_terminal_output_digest: row
            .try_get("consumer_terminal_output_digest")
            .map_err(storage_error)?,
        resolution_action: row
            .try_get::<Option<String>, _>("resolution_action")
            .map_err(storage_error)?
            .map(|value| decode(serde_json::Value::String(value)))
            .transpose()?,
        resolution_reason: row.try_get("resolution_reason").map_err(storage_error)?,
    })
}
