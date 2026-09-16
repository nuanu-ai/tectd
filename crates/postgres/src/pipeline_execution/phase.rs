use super::*;
pub(super) type LockedRun = (
    Uuid,
    Uuid,
    i64,
    i64,
    String,
    String,
    String,
    serde_json::Value,
    String,
    Option<String>,
    Option<i32>,
    Option<Uuid>,
    Option<String>,
);

#[allow(clippy::too_many_arguments)]
pub(crate) async fn complete_phase(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &CompletePipelinePhase,
) -> Result<PipelineMutationOutcome> {
    phase_validation::validate_output_integrity(&request.output)?;
    let payload = json(request)?;
    if let Some((stored, result, erased)) = sqlx::query_as::<_, (Option<serde_json::Value>, Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_phase_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    )
    .bind(tenant).bind(workspace).bind(request.request_id)
    .fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased{return Err(Error::KnowledgePayloadErased)}
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    // DK lock order: workspace knowledge state precedes the run lock.
    let _ = crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let run_row:LockedRun=sqlx::query_as(
        "SELECT scope_id,slice_id,slice_revision,revision,status,definition_version,definition_digest,definition,delivery_mode,current_phase_id,current_phase_ordinal,knowledge_manifest_id,knowledge_manifest_digest FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    if let Some((stored, result, erased)) = sqlx::query_as::<_, (Option<serde_json::Value>, Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_phase_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    )
    .bind(tenant).bind(workspace).bind(request.request_id)
    .fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased{return Err(Error::KnowledgePayloadErased)}
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    if run_row.3 != request.run_revision {
        return Err(Error::StaleRevision);
    }
    if matches!(run_row.4.as_str(), "completed" | "escalated") {
        return Err(Error::Forbidden);
    }
    if run_row.9.as_deref() != Some(&request.phase_id) {
        return Err(Error::StaleContext);
    }
    checkpoint::ensure_run_source_open(tx, tenant, workspace, request.run_id).await?;
    let definition: PipelineDefinitionSnapshot = decode(run_row.7.clone())?;
    let phase = definition
        .phases
        .iter()
        .find(|phase| phase.id == request.phase_id)
        .ok_or(Error::InternalInvariant)?;
    let open_checkpoint = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pipeline_research_checkpoints WHERE tenant_id=$1 AND workspace_id=$2 AND producer_run_id=$3 AND status='open')",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let checkpoint_rework = definition.kind == PipelineKind::DeepBrainstorming
        && phase.ordinal == 5
        && request.outcome == PipelinePhaseOutcome::Completed
        && request.transition == PipelineTransition::Continue
        && request.research_checkpoint.is_none()
        && request.revisit_phase_id.as_ref().is_some_and(|target| {
            definition
                .phases
                .iter()
                .any(|candidate| &candidate.id == target && candidate.ordinal <= 4)
        });
    if open_checkpoint && !checkpoint_rework {
        return Err(Error::InputPending);
    }
    inquiry_contract::validate_phase(
        tx,
        tenant,
        workspace,
        request,
        definition.kind.as_str(),
        phase.ordinal,
    )
    .await?;
    knowledge_publication::validate(
        tx,
        tenant,
        workspace,
        session,
        request.run_id,
        phase,
        &request.output,
    )
    .await?;
    let knowledge =
        phase_validation::load_manifest(tx, tenant, workspace, session, run_row.11).await?;
    crate::durable_knowledge::manifest::validate_completion(
        tx,
        tenant,
        workspace,
        request.run_id,
        run_row.0,
        run_row.1,
        &phase.id,
        session,
        knowledge.as_ref(),
        request.consumed_knowledge.as_ref(),
    )
    .await?;
    validate_consumed_outputs(
        tx,
        tenant,
        workspace,
        request.run_id,
        phase.ordinal,
        &request.consumed_outputs,
    )
    .await?;
    if definition.kind == PipelineKind::FullDesignToExecution
        && phase.id == "slice-reconciliation-runner"
        && phase
            .required_artifacts
            .iter()
            .any(|artifact| artifact.name_pattern == "requirements-ledger.json")
    {
        helpers::validate_reconciliation_ledger_lineage(
            tx,
            tenant,
            workspace,
            request.run_id,
            &request.output,
        )
        .await?;
    }
    validate_review_authorization(
        tx,
        tenant,
        workspace,
        request.run_id,
        &definition,
        phase,
        &request.output,
    )
    .await?;
    validate_consumed_inputs(
        tx,
        tenant,
        workspace,
        request.run_id,
        &phase.id,
        &request.consumed_inputs,
    )
    .await?;
    validate_reviewer_boundary(tx, tenant, workspace, request.run_id, phase, request).await?;
    enforce_retry_policy(tx, tenant, workspace, request.run_id, phase).await?;
    let attempt:i64=sqlx::query_scalar("SELECT COALESCE(MAX(attempt),0)+1 FROM slice_pipeline_phase_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(&request.phase_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let output_revision:i64=sqlx::query_scalar("SELECT COALESCE(MAX(revision),0)+1 FROM slice_pipeline_phase_outputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(&request.phase_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let attempt_id = Uuid::new_v4();
    let output_id = Uuid::new_v4();
    let output_digest = digest(&request.output)?;
    let reviewer_context = request
        .output
        .reviewer_context
        .as_ref()
        .map(json)
        .transpose()?;
    let followup_proposal = request
        .output
        .followup_proposal
        .as_ref()
        .map(json)
        .transpose()?;
    sqlx::query("INSERT INTO slice_pipeline_phase_attempts(id,tenant_id,workspace_id,run_id,phase_id,phase_ordinal,attempt,outcome,transition,revisit_phase_id,escalation_target,actor_session_id,reviewer_context,request_id,request_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)")
        .bind(attempt_id).bind(tenant).bind(workspace).bind(request.run_id).bind(&request.phase_id).bind(phase.ordinal as i32).bind(attempt)
        .bind(enum_text(&request.outcome)?).bind(enum_text(&request.transition)?).bind(&request.revisit_phase_id)
        .bind(request.escalation_target.map(PipelineKind::as_str)).bind(session).bind(reviewer_context)
        .bind(request.request_id).bind(&payload).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO slice_pipeline_phase_outputs(id,tenant_id,workspace_id,run_id,attempt_id,phase_id,phase_ordinal,revision,body,producer_context_id,body_digest,reference,fields,verdict,dispositions,skill_reads,resource_reads,artifacts,validator_receipts,followup_proposal,knowledge_publication) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21)")
        .bind(output_id).bind(tenant).bind(workspace).bind(request.run_id).bind(attempt_id).bind(&request.phase_id).bind(phase.ordinal as i32)
        .bind(output_revision).bind(&request.output.body).bind(&request.output.producer_context_id).bind(&output_digest).bind(&request.output.reference).bind(json(&request.output.fields)?)
        .bind(&request.output.verdict).bind(json(&request.output.dispositions)?).bind(json(&request.output.skill_reads)?)
        .bind(json(&request.output.resource_reads)?).bind(json(&request.output.artifacts)?)
        .bind(json(&request.output.validator_receipts)?)
        .bind(followup_proposal)
        .bind(request.output.knowledge_publication.as_ref().map(json).transpose()?)
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO slice_pipeline_output_bindings(tenant_id,workspace_id,run_id,phase_id,phase_ordinal,output_id,output_revision) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(tenant_id,workspace_id,run_id,phase_id) DO UPDATE SET output_id=EXCLUDED.output_id,output_revision=EXCLUDED.output_revision,stale=false,stale_reason=NULL,updated_at=pg_catalog.clock_timestamp()")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(&request.phase_id).bind(phase.ordinal as i32).bind(output_id).bind(output_revision)
        .execute(&mut **tx).await.map_err(storage_error)?;
    let (status, next_id, next_ordinal) =
        next_state(tx, tenant, workspace, session, request, &definition, phase).await?;
    let next_revision = run_row.3.checked_add(1).ok_or(Error::StorageUnavailable)?;
    let created_checkpoint = checkpoint::create(
        tx,
        tenant,
        workspace,
        request,
        &run_row,
        attempt_id,
        output_id,
        output_revision,
        &output_digest,
        next_revision,
    )
    .await?;
    sqlx::query("UPDATE slice_pipeline_runs SET revision=$4,status=$5,current_phase_id=$6,current_phase_ordinal=$7 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(next_revision).bind(status).bind(next_id.as_deref()).bind(next_ordinal.map(|value| value as i32))
        .execute(&mut **tx).await.map_err(storage_error)?;
    if let Some(next_phase) = next_id.as_deref() {
        let manifest = crate::durable_knowledge::manifest::capture(
            tx,
            tenant,
            workspace,
            request.run_id,
            next_revision,
            run_row.0,
            run_row.1,
            next_phase,
            session,
        )
        .await?;
        sqlx::query("UPDATE slice_pipeline_runs SET knowledge_manifest_id=$4,knowledge_manifest_digest=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(request.run_id).bind(manifest.as_ref().map(|v|v.id)).bind(manifest.as_ref().map(|v|v.digest.as_str())).execute(&mut **tx).await.map_err(storage_error)?;
    } else {
        sqlx::query("UPDATE slice_pipeline_runs SET knowledge_manifest_id=NULL,knowledge_manifest_digest=NULL WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(request.run_id).execute(&mut **tx).await.map_err(storage_error)?;
    }
    let result = match request.transition {
        PipelineTransition::Complete => Some(
            publish_result(
                tx,
                tenant,
                workspace,
                session,
                request,
                attempt_id,
                &run_row,
                "managed_completed",
                SliceResultOutcome::Completed,
            )
            .await?,
        ),
        PipelineTransition::Escalate => Some(
            publish_result(
                tx,
                tenant,
                workspace,
                session,
                request,
                attempt_id,
                &run_row,
                "managed_escalated",
                SliceResultOutcome::Blocked,
            )
            .await?,
        ),
        PipelineTransition::Block if request.publish_blocked_result => Some(
            publish_result(
                tx,
                tenant,
                workspace,
                session,
                request,
                attempt_id,
                &run_row,
                "managed_blocked",
                SliceResultOutcome::Blocked,
            )
            .await?,
        ),
        _ => None,
    };
    let planning_input = if let Some(result) = &result {
        sqlx::query_scalar("SELECT id FROM slice_planning_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND source_result_id=$3")
            .bind(tenant).bind(workspace).bind(result.id).fetch_optional(&mut **tx).await.map_err(storage_error)?
    } else {
        None
    };
    crate::knowledge_lifecycle::erase::register_pipeline_phase_copies(
        tx,
        tenant,
        workspace,
        request.run_id,
        run_row.11,
        attempt_id,
        output_id,
        result.as_ref().map(|value| value.id),
        planning_input,
    )
    .await?;
    if let Some(checkpoint) = created_checkpoint {
        crate::knowledge_lifecycle::erase::register_checkpoint_copies(
            tx,
            tenant,
            workspace,
            checkpoint.checkpoint_id,
            attempt_id,
        )
        .await?;
    }
    let principal = session_principal(tx, session).await?;
    let outcome = PipelineMutationOutcome {
        context: load_context(tx, tenant, workspace, principal, request.run_id)
            .await?
            .ok_or(Error::InternalInvariant)?,
        result,
    };
    sqlx::query("UPDATE slice_pipeline_phase_attempts SET result_payload=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(attempt_id).bind(json(&outcome)?).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(outcome)
}

mod helpers;

use helpers::{
    enforce_retry_policy, next_state, publish_result, validate_consumed_inputs,
    validate_consumed_outputs, validate_review_authorization, validate_reviewer_boundary,
};
