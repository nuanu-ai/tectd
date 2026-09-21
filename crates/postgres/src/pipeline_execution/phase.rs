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
    phase_validation::validate_ready_evidence_artifacts(
        tx,
        tenant,
        workspace,
        &request.output.evidence_artifacts,
    )
    .await?;
    // Resolve and validate navigation before result-lineage checks. A permitted
    // backward transition is the recovery path for legacy malformed outputs.
    let planned_next = helpers::plan_next_state(request, &definition, phase)?;
    if definition.kind == PipelineKind::FullDesignToExecution
        && matches!(
            phase.id.as_str(),
            "slice-component-decision-interrogator" | "slice-reconciliation-runner"
        )
        && phase
            .required_artifacts
            .iter()
            .any(|artifact| artifact.name_pattern == "requirements-ledger.json")
    {
        if phase.id == "slice-component-decision-interrogator" {
            helpers::validate_decision_requirements_ledger(&request.output)?;
            input::validate_source_amendment_ledger(
                tx,
                tenant,
                workspace,
                request.run_id,
                &request.output,
            )
            .await?;
        } else if planned_next.revisit_ordinal.is_some() {
            // The submitted phase 7 output must remain valid, but recovery must
            // not depend on parsing the legacy phase 5 output being replaced.
            helpers::validate_reconciliation_requirements_ledger(&request.output)?;
        } else {
            helpers::validate_reconciliation_ledger_lineage(
                tx,
                tenant,
                workspace,
                request.run_id,
                &request.output,
            )
            .await?;
        }
    }
    phase_validation::validate_output_integrity(&request.output, &run_row.5)?;
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
    if !run_row.5.starts_with("0.7") {
        validate_consumed_outputs(
            tx,
            tenant,
            workspace,
            request.run_id,
            phase.ordinal,
            &request.consumed_outputs,
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
    if !run_row.5.starts_with("0.7") {
        validate_consumed_inputs(
            tx,
            tenant,
            workspace,
            request.run_id,
            &phase.id,
            &request.consumed_inputs,
        )
        .await?;
    }
    // Resolve proof pointers from backend-owned rows only after all caller
    // declarations have matched the current bindings.  The submitted digest
    // and read receipts are validation inputs, never persisted as proof.
    let (evidence_refs, knowledge_binding) = backend_evidence_refs(
        tx,
        tenant,
        workspace,
        request.run_id,
        &request.phase_id,
        phase.ordinal,
        &request.consumed_knowledge,
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
    sqlx::query("INSERT INTO slice_pipeline_phase_attempts(id,tenant_id,workspace_id,run_id,phase_id,phase_ordinal,attempt,outcome,transition,revisit_phase_id,escalation_target,actor_session_id,reviewer_context,request_id,request_payload,evidence_refs,knowledge_manifest_id,knowledge_manifest_digest,knowledge_workspace_generation,stale_dependency,stale_reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,false,NULL)")
        .bind(attempt_id).bind(tenant).bind(workspace).bind(request.run_id).bind(&request.phase_id).bind(phase.ordinal as i32).bind(attempt)
        .bind(enum_text(&request.outcome)?).bind(enum_text(&request.transition)?).bind(&request.revisit_phase_id)
        .bind(request.escalation_target.map(PipelineKind::as_str)).bind(session).bind(reviewer_context)
        .bind(request.request_id).bind(&payload).bind(&evidence_refs)
        .bind(knowledge_binding.as_ref().map(|value| value.manifest_id))
        .bind(knowledge_binding.as_ref().map(|value| value.digest.as_str()))
        .bind(knowledge_binding.as_ref().map(|value| value.workspace_generation))
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO slice_pipeline_phase_outputs(id,tenant_id,workspace_id,run_id,attempt_id,phase_id,phase_ordinal,revision,body,producer_context_id,body_digest,reference,fields,verdict,dispositions,skill_reads,resource_reads,artifacts,evidence_artifacts,validator_receipts,followup_proposal,knowledge_publication) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22)")
        .bind(output_id).bind(tenant).bind(workspace).bind(request.run_id).bind(attempt_id).bind(&request.phase_id).bind(phase.ordinal as i32)
        .bind(output_revision).bind(&request.output.body).bind(&request.output.producer_context_id).bind(&output_digest).bind(&request.output.reference).bind(json(&request.output.fields)?)
        .bind(&request.output.verdict).bind(json(&request.output.dispositions)?).bind(json(&request.output.skill_reads)?)
        .bind(json(&request.output.resource_reads)?).bind(json(&request.output.artifacts)?)
        .bind(json(&request.output.evidence_artifacts)?)
        .bind(json(&request.output.validator_receipts)?)
        .bind(followup_proposal)
        .bind(request.output.knowledge_publication.as_ref().map(json).transpose()?)
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO slice_pipeline_output_bindings(tenant_id,workspace_id,run_id,phase_id,phase_ordinal,output_id,output_revision) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(tenant_id,workspace_id,run_id,phase_id) DO UPDATE SET output_id=EXCLUDED.output_id,output_revision=EXCLUDED.output_revision,stale=false,stale_reason=NULL,updated_at=pg_catalog.clock_timestamp()")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(&request.phase_id).bind(phase.ordinal as i32).bind(output_id).bind(output_revision)
        .execute(&mut **tx).await.map_err(storage_error)?;
    helpers::apply_rework(
        tx,
        tenant,
        workspace,
        session,
        request.run_id,
        &planned_next,
    )
    .await?;
    let status = planned_next.status;
    let next_id = planned_next.next_id;
    let next_ordinal = planned_next.next_ordinal;
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
    let outcome_payload = json(&outcome)?;
    sqlx::query("UPDATE slice_pipeline_phase_attempts SET result_payload=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(attempt_id).bind(outcome_payload).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(outcome)
}

pub(super) mod helpers;

async fn backend_evidence_refs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase_id: &str,
    phase_ordinal: u32,
    consumed_knowledge: &Option<ConsumedKnowledgeManifestRef>,
) -> Result<(serde_json::Value, Option<PipelineKnowledgeBindingReceipt>)> {
    let outputs: Vec<(String, i64, Uuid, String)> = sqlx::query_as(
        "SELECT b.phase_id,b.output_revision,o.id,o.body_digest FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_ordinal < $4 AND b.stale=false AND NOT o.payload_erased ORDER BY b.phase_ordinal",
    )
    .bind(tenant).bind(workspace).bind(run).bind(phase_ordinal as i32)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let inputs: Vec<(Uuid, i64, String)> = sqlx::query_as(
        "SELECT id,sequence,input_digest FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4 AND NOT payload_erased ORDER BY sequence",
    )
    .bind(tenant).bind(workspace).bind(run).bind(phase_id)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut refs = Vec::with_capacity(outputs.len() + inputs.len() + 1);
    refs.extend(outputs.into_iter().map(|(phase, revision, id, digest)| {
        serde_json::json!({
            "kind":"output", "reference":id, "phase_id":phase, "revision":revision, "digest":digest
        })
    }));
    refs.extend(inputs.into_iter().map(|(id, sequence, digest)| {
        serde_json::json!({
            "kind":"input", "reference":id, "sequence":sequence, "digest":digest
        })
    }));
    let binding = if let Some(consumed) = consumed_knowledge {
        let row: Option<(Uuid, String, i64)> = sqlx::query_as(
            "SELECT id,digest,workspace_generation FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND digest=$4 AND run_id=$5 AND NOT payload_erased",
        )
        .bind(tenant).bind(workspace).bind(consumed.manifest_id).bind(&consumed.digest).bind(run)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
        let (manifest_id, digest, generation) = row.ok_or(Error::StaleContext)?;
        refs.push(serde_json::json!({"kind":"knowledge_manifest","reference":manifest_id,"revision":generation,"digest":digest}));
        Some(PipelineKnowledgeBindingReceipt {
            manifest_id,
            digest,
            workspace_generation: generation,
        })
    } else {
        None
    };
    Ok((serde_json::Value::Array(refs), binding))
}

use helpers::{
    enforce_retry_policy, publish_result, validate_consumed_inputs, validate_consumed_outputs,
    validate_review_authorization, validate_reviewer_boundary,
};
