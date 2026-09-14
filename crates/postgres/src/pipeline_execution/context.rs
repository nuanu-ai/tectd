use super::*;

#[allow(clippy::type_complexity)]
pub(crate) async fn load_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run_id: Uuid,
) -> Result<Option<PipelineRunContext>> {
    let row:Option<(Uuid,Uuid,Uuid,i64,i64,String,String,String,serde_json::Value,String,String,String,Option<String>,Option<i32>,Option<Uuid>)>=sqlx::query_as(
        "SELECT id,scope_id,slice_id,slice_revision,revision,definition_kind,definition_version,definition_digest,definition,delivery_mode,qualification_reason,status,current_phase_id,current_phase_ordinal,knowledge_manifest_id FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    let definition: PipelineDefinitionSnapshot = decode(row.8)?;
    let run = PipelineRun {
        id: row.0,
        scope_id: row.1,
        slice_id: row.2,
        slice_revision: row.3,
        revision: row.4,
        definition_kind: pipeline(&row.5)?,
        definition_version: row.6,
        definition_digest: row.7,
        delivery_mode: mode(&row.9)?,
        qualification_reason: row.10,
        status: run_status(&row.11)?,
        current_phase_id: row.12,
        current_phase_ordinal: row.13.map(|value| value as u32),
    };
    let attempt_rows:Vec<(Uuid,String,i32,i64,String,String,i64,Uuid,String,Option<String>,Uuid,Option<serde_json::Value>)>=sqlx::query_as(
        "SELECT a.id,a.phase_id,a.phase_ordinal,a.attempt,a.outcome,a.transition,o.revision,o.id,o.body_digest,o.reference,a.actor_session_id,a.reviewer_context FROM slice_pipeline_phase_attempts a JOIN slice_pipeline_phase_outputs o ON o.tenant_id=a.tenant_id AND o.workspace_id=a.workspace_id AND o.attempt_id=a.id WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.run_id=$3 ORDER BY a.created_at,a.id")
        .bind(tenant).bind(workspace).bind(run_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let attempts = attempt_rows
        .into_iter()
        .map(|row| {
            Ok(PipelinePhaseAttempt {
                id: row.0,
                run_id,
                phase_id: row.1,
                phase_ordinal: row.2 as u32,
                attempt: row.3,
                outcome: phase_outcome(&row.4)?,
                transition: transition(&row.5)?,
                output_revision: row.6,
                output_id: row.7,
                output_digest: row.8,
                output_reference: row.9,
                actor_session_id: row.10,
                reviewer_context: row.11.map(decode).transpose()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let binding_rows:Vec<(String,i32,i64,Uuid,String,Option<String>,bool,Option<String>)>=sqlx::query_as(
        "SELECT b.phase_id,b.phase_ordinal,b.output_revision,o.id,o.body_digest,o.reference,b.stale,b.stale_reason FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 ORDER BY b.phase_ordinal")
        .bind(tenant).bind(workspace).bind(run_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let bindings = binding_rows
        .into_iter()
        .map(|row| PipelineOutputBinding {
            phase_id: row.0,
            phase_ordinal: row.1 as u32,
            output_revision: row.2,
            output_id: row.3,
            output_digest: row.4,
            reference: row.5,
            stale: row.6,
            stale_reason: row.7,
        })
        .collect();
    let output_rows:Vec<serde_json::Value>=sqlx::query_scalar(
        "SELECT pg_catalog.jsonb_build_object('id',o.id,'run_id',o.run_id,'phase_id',o.phase_id,'phase_ordinal',o.phase_ordinal,'revision',o.revision,'body',o.body,'producer_context_id',o.producer_context_id,'digest',o.body_digest,'reference',o.reference,'fields',o.fields,'verdict',o.verdict,'dispositions',o.dispositions,'skill_reads',o.skill_reads,'resource_reads',o.resource_reads,'artifacts',o.artifacts,'validator_receipts',o.validator_receipts,'followup_proposal',o.followup_proposal,'stale',b.stale,'stale_reason',b.stale_reason) FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 ORDER BY b.phase_ordinal")
        .bind(tenant).bind(workspace).bind(run_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let outputs = output_rows
        .into_iter()
        .map(decode)
        .collect::<Result<Vec<_>>>()?;
    let input_rows:Vec<(Uuid,i64,String,String,String,Uuid)>=sqlx::query_as(
        "SELECT id,sequence,phase_id,input,input_digest,actor_session_id FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 ORDER BY sequence")
        .bind(tenant).bind(workspace).bind(run_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let inputs = input_rows
        .into_iter()
        .map(|row| PipelineInput {
            id: row.0,
            sequence: row.1,
            phase_id: row.2,
            input: row.3,
            digest: row.4,
            actor_session_id: row.5,
        })
        .collect();
    let result_row:Option<(Uuid,Uuid,i64,i64,String,String,serde_json::Value,String,String,String,Option<Uuid>,Option<String>,Option<String>,Option<Uuid>,Option<String>)>=sqlx::query_as(
        "SELECT id,slice_id,slice_revision,revision,outcome,summary,evidence,scope_impact,remaining_work,provenance,pipeline_run_id,pipeline_definition_version,pipeline_definition_digest,pipeline_final_attempt_id,pipeline_result_origin FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND pipeline_run_id=$3 ORDER BY revision DESC LIMIT 1")
        .bind(tenant).bind(workspace).bind(run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let result = result_row
        .map(|row| {
            Ok(SliceResult {
                id: row.0,
                slice_id: row.1,
                slice_revision: row.2,
                revision: row.3,
                outcome: decode(serde_json::Value::String(row.4))?,
                summary: row.5,
                evidence: decode(row.6)?,
                scope_impact: row.7,
                remaining_work: row.8,
                provenance: row.9,
                pipeline_run_id: row.10,
                pipeline_definition_version: row.11,
                pipeline_definition_digest: row.12,
                pipeline_final_attempt_id: row.13,
                pipeline_result_origin: row.14,
            })
        })
        .transpose()?;
    let delivered_phases = match run.delivery_mode {
        PipelineDeliveryMode::Whole => definition.phases.clone(),
        PipelineDeliveryMode::Phasewise => run
            .current_phase_id
            .as_ref()
            .and_then(|id| {
                definition
                    .phases
                    .iter()
                    .find(|phase| &phase.id == id)
                    .cloned()
            })
            .into_iter()
            .collect(),
    };
    let knowledge = crate::durable_knowledge::manifest::load(tx, tenant, workspace, row.14).await?;
    let knowledge_status = crate::durable_knowledge::manifest::status(
        tx,
        tenant,
        workspace,
        run.id,
        run.scope_id,
        run.slice_id,
        run.current_phase_id.as_deref(),
        knowledge.as_ref(),
    )
    .await?;
    Ok(Some(PipelineRunContext {
        run,
        definition,
        delivered_phases,
        attempts,
        bindings,
        outputs,
        outputs_complete: true,
        inputs,
        result,
        knowledge,
        knowledge_status,
    }))
}

pub(crate) async fn load_output(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run_id: Uuid,
    output_id: Uuid,
    digest: &str,
) -> Result<Option<PipelinePhaseOutput>> {
    let row:Option<serde_json::Value>=sqlx::query_scalar(
        "SELECT pg_catalog.jsonb_build_object('id',o.id,'run_id',o.run_id,'phase_id',o.phase_id,'phase_ordinal',o.phase_ordinal,'revision',o.revision,'body',o.body,'producer_context_id',o.producer_context_id,'digest',o.body_digest,'reference',o.reference,'fields',o.fields,'verdict',o.verdict,'dispositions',o.dispositions,'skill_reads',o.skill_reads,'resource_reads',o.resource_reads,'artifacts',o.artifacts,'validator_receipts',o.validator_receipts,'followup_proposal',o.followup_proposal,'stale',COALESCE(b.stale,true),'stale_reason',CASE WHEN b.output_id IS NULL THEN 'not_current_binding' ELSE b.stale_reason END) FROM slice_pipeline_phase_outputs o LEFT JOIN slice_pipeline_output_bindings b ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 AND o.id=$4 AND o.body_digest=$5")
        .bind(tenant).bind(workspace).bind(run_id).bind(output_id).bind(digest)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    row.map(decode).transpose()
}
