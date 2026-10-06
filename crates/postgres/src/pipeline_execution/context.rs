use super::*;

mod authorization;
mod delivery_receipt;

use authorization::authorize_context_copies;
pub(super) use authorization::{authorize_run_origin, authorize_run_origin_if_present};

#[derive(serde::Deserialize)]
struct StoredRunRow {
    id: Uuid,
    scope_id: Uuid,
    slice_id: Uuid,
    slice_revision: i64,
    revision: i64,
    definition_kind: String,
    definition_version: String,
    definition_digest: String,
    selected_option_id: Option<String>,
    verification_plan_id: Option<String>,
    verification_plan_version: Option<String>,
    verification_plan_digest: Option<String>,
    definition: serde_json::Value,
    delivery_mode: String,
    qualification_reason: Option<String>,
    status: String,
    current_phase_id: Option<String>,
    current_phase_ordinal: Option<i32>,
    knowledge_manifest_id: Option<Uuid>,
    payload_erased: bool,
    inquiry: Option<serde_json::Value>,
    source_checkpoint_id: Option<Uuid>,
    source_checkpoint_digest: Option<String>,
}

#[derive(sqlx::FromRow)]
struct StoredAttemptRow {
    id: Uuid,
    phase_id: String,
    phase_ordinal: i32,
    attempt: i64,
    outcome: String,
    transition: String,
    output_revision: i64,
    output_id: Uuid,
    output_digest: String,
    output_reference: Option<String>,
    actor_session_id: Uuid,
    reviewer_context: Option<serde_json::Value>,
    evidence_refs: serde_json::Value,
    knowledge_manifest_id: Option<Uuid>,
    knowledge_manifest_digest: Option<String>,
    knowledge_workspace_generation: Option<i64>,
    stale_dependency: bool,
    stale_reason: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredEvidenceRef {
    kind: String,
    reference: String,
    digest: String,
    #[serde(default)]
    revision: Option<i64>,
    #[serde(default)]
    #[serde(rename = "phase_id")]
    _phase_id: Option<String>,
    #[serde(default)]
    #[serde(rename = "sequence")]
    _sequence: Option<i64>,
}

fn decode_evidence_refs(value: serde_json::Value) -> Result<Vec<PipelineEvidenceRef>> {
    let refs: Vec<StoredEvidenceRef> = decode(value)?;
    Ok(refs
        .into_iter()
        .map(
            |StoredEvidenceRef {
                 kind,
                 reference,
                 digest,
                 revision,
                 _phase_id: _,
                 _sequence: _,
             }| PipelineEvidenceRef {
                kind,
                reference,
                digest,
                revision,
            },
        )
        .collect())
}

#[allow(clippy::type_complexity)]
pub(crate) async fn load_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
) -> Result<Option<PipelineRunContext>> {
    load_context_with_delivery_receipt(tx, tenant, workspace, principal, run_id, true, true, None)
        .await
}

pub(crate) async fn load_context_without_delivery_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
) -> Result<Option<PipelineRunContext>> {
    load_context_with_delivery_receipt(tx, tenant, workspace, principal, run_id, false, true, None)
        .await
}

/// Completion preflight keeps all context authorization and decoding. Its
/// resource status is recomputed under the workspace lock by phase completion.
pub(crate) async fn load_completion_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
) -> Result<Option<PipelineRunContext>> {
    load_context_with_delivery_receipt(tx, tenant, workspace, principal, run_id, true, false, None)
        .await
}

/// Input preflight keeps context authorization, decoding, and legacy status.
/// The caller discards this context and verifies generic resource status after
/// acquiring both the workspace knowledge lock and the run lock.
pub(crate) async fn load_input_preflight_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
) -> Result<Option<PipelineRunContext>> {
    load_context_with_delivery_receipt(tx, tenant, workspace, principal, run_id, true, false, None)
        .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn load_context_with_proofs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
    session: Uuid,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<Option<PipelineRunContext>> {
    load_context_with_delivery_receipt(
        tx,
        tenant,
        workspace,
        principal,
        run_id,
        true,
        true,
        Some((session, proofs)),
    )
    .await
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
async fn load_context_with_delivery_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
    issue_delivery_receipt: bool,
    verify_resource_status: bool,
    mut proofs: Option<(Uuid, &mut crate::knowledge_lifecycle::PublicationProofScope)>,
) -> Result<Option<PipelineRunContext>> {
    tect_application::request_diagnostics::measure("pg.context_assembly", async {
    if let Some((session, proofs)) = proofs.as_ref() {
        proofs.require_identity(tenant, workspace, principal, *session)?;
    }
    let row:Option<serde_json::Value>=sqlx::query_scalar(
        "SELECT pg_catalog.jsonb_build_object('id',id,'scope_id',scope_id,'slice_id',slice_id,'slice_revision',slice_revision,'revision',revision,'definition_kind',definition_kind,'definition_version',definition_version,'definition_digest',definition_digest,'selected_option_id',selected_option_id,'verification_plan_id',verification_plan_id,'verification_plan_version',verification_plan_version,'verification_plan_digest',verification_plan_digest,'definition',definition,'delivery_mode',delivery_mode,'qualification_reason',qualification_reason,'status',status,'current_phase_id',current_phase_id,'current_phase_ordinal',current_phase_ordinal,'knowledge_manifest_id',knowledge_manifest_id,'payload_erased',payload_erased,'inquiry',inquiry,'source_checkpoint_id',source_checkpoint_id,'source_checkpoint_digest',source_checkpoint_digest) FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    let row: StoredRunRow = decode(row)?;
    tect_application::request_diagnostics::measure("pg.context_authorization",
        authorize_context_copies(tx, tenant, workspace, principal, run_id)).await?;
    if row.payload_erased {
        return Err(Error::KnowledgePayloadErased);
    }
    let definition: PipelineDefinitionSnapshot = decode(row.definition)?;
    let run = PipelineRun {
        id: row.id,
        scope_id: row.scope_id,
        slice_id: row.slice_id,
        slice_revision: row.slice_revision,
        revision: row.revision,
        definition_kind: pipeline(&row.definition_kind)?,
        definition_version: row.definition_version,
        definition_digest: row.definition_digest,
        selected_option_id: row.selected_option_id,
        verification_plan_id: row.verification_plan_id,
        verification_plan_version: row.verification_plan_version,
        verification_plan_digest: row.verification_plan_digest,
        delivery_mode: mode(&row.delivery_mode)?,
        qualification_reason: row.qualification_reason.ok_or(Error::InternalInvariant)?,
        status: run_status(&row.status)?,
        current_phase_id: row.current_phase_id,
        current_phase_ordinal: row.current_phase_ordinal.map(|value| value as u32),
    };
    let inquiry = row.inquiry.map(decode).transpose()?;
    let source_checkpoint = row
        .source_checkpoint_id
        .map(|checkpoint_id| {
            Ok(PipelineCheckpointRef {
                checkpoint_id,
                digest: row
                    .source_checkpoint_digest
                    .clone()
                    .ok_or(Error::InternalInvariant)?,
            })
        })
        .transpose()?;
    let attempt_rows:Vec<StoredAttemptRow>=sqlx::query_as(
        "SELECT a.id,a.phase_id,a.phase_ordinal,a.attempt,a.outcome,a.transition,o.revision AS output_revision,o.id AS output_id,o.body_digest AS output_digest,o.reference AS output_reference,a.actor_session_id,a.reviewer_context,a.evidence_refs,a.knowledge_manifest_id,a.knowledge_manifest_digest,a.knowledge_workspace_generation,a.stale_dependency,a.stale_reason FROM slice_pipeline_phase_attempts a JOIN slice_pipeline_phase_outputs o ON o.tenant_id=a.tenant_id AND o.workspace_id=a.workspace_id AND o.attempt_id=a.id WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.run_id=$3 AND NOT a.payload_erased AND NOT o.payload_erased ORDER BY a.created_at,a.id")
        .bind(tenant).bind(workspace).bind(run_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let attempts = attempt_rows
        .into_iter()
        .map(|row| {
            Ok(PipelinePhaseAttempt {
                id: row.id,
                run_id,
                phase_id: row.phase_id,
                phase_ordinal: row.phase_ordinal as u32,
                attempt: row.attempt,
                outcome: phase_outcome(&row.outcome)?,
                transition: transition(&row.transition)?,
                output_revision: row.output_revision,
                output_id: row.output_id,
                output_digest: row.output_digest,
                output_reference: row.output_reference,
                actor_session_id: row.actor_session_id,
                reviewer_context: row.reviewer_context.map(decode).transpose()?,
                evidence_refs: decode_evidence_refs(row.evidence_refs)?,
                knowledge_binding: row
                    .knowledge_manifest_id
                    .map(|manifest_id| {
                        Ok(PipelineKnowledgeBindingReceipt {
                            manifest_id,
                            digest: row
                                .knowledge_manifest_digest
                                .clone()
                                .ok_or(Error::InternalInvariant)?,
                            workspace_generation: row
                                .knowledge_workspace_generation
                                .ok_or(Error::InternalInvariant)?,
                        })
                    })
                    .transpose()?,
                stale_dependency: row.stale_dependency,
                stale_reason: row.stale_reason,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let binding_rows:Vec<(String,i32,i64,Uuid,String,Option<String>,bool,Option<String>)>=sqlx::query_as(
        "SELECT b.phase_id,b.phase_ordinal,b.output_revision,o.id,o.body_digest,o.reference,b.stale,b.stale_reason FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND NOT o.payload_erased ORDER BY b.phase_ordinal")
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
        "SELECT pg_catalog.jsonb_build_object('id',o.id,'run_id',o.run_id,'phase_id',o.phase_id,'phase_ordinal',o.phase_ordinal,'revision',o.revision,'body',o.body,'producer_context_id',o.producer_context_id,'digest',o.body_digest,'reference',o.reference,'knowledge_publication',o.knowledge_publication,'fields',o.fields,'verdict',o.verdict,'dispositions',o.dispositions,'skill_reads',o.skill_reads,'resource_reads',o.resource_reads,'artifacts',o.artifacts,'evidence_artifacts',o.evidence_artifacts,'validator_receipts',o.validator_receipts,'followup_proposal',o.followup_proposal,'stale',b.stale,'stale_reason',b.stale_reason) FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND NOT o.payload_erased ORDER BY b.phase_ordinal")
        .bind(tenant).bind(workspace).bind(run_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let outputs = output_rows
        .into_iter()
        .map(decode)
        .collect::<Result<Vec<_>>>()?;
    let input_rows:Vec<(Uuid,i64,String,String,String,Uuid,Option<serde_json::Value>)>=sqlx::query_as(
        "SELECT id,sequence,phase_id,input,input_digest,actor_session_id,request_payload->'source_amendment' FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND NOT payload_erased ORDER BY sequence")
        .bind(tenant).bind(workspace).bind(run_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let inputs = input_rows
        .into_iter()
        .map(|row| {
            Ok(PipelineInput {
                id: row.0,
                sequence: row.1,
                phase_id: row.2,
                input: row.3,
                digest: row.4,
                actor_session_id: row.5,
                source_amendment: row.6.map(decode).transpose()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let result_row:Option<(Uuid,Uuid,i64,i64,String,String,serde_json::Value,String,String,String,Option<Uuid>,Option<String>,Option<String>,Option<Uuid>,Option<String>)>=sqlx::query_as(
        "SELECT id,slice_id,slice_revision,revision,outcome,summary,evidence,scope_impact,remaining_work,provenance,pipeline_run_id,pipeline_definition_version,pipeline_definition_digest,pipeline_final_attempt_id,pipeline_result_origin FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND pipeline_run_id=$3 AND NOT payload_erased ORDER BY revision DESC LIMIT 1")
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
                knowledge_provenance: None,
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
    let knowledge = crate::durable_knowledge::manifest::load(
        tx,
        tenant,
        workspace,
        row.knowledge_manifest_id,
        principal,
    )
    .await?;
    let knowledge_status = crate::durable_knowledge::manifest::status(
        tx,
        tenant,
        workspace,
        principal,
        run.id,
        run.scope_id,
        run.slice_id,
        run.current_phase_id.as_deref(),
        knowledge.as_ref(),
    )
    .await?;
    let knowledge_resources = crate::durable_knowledge::manifest::load_resources(
        tx,
        tenant,
        workspace,
        row.knowledge_manifest_id,
        principal,
    )
    .await?;
    let knowledge_resource_status = if verify_resource_status {
        match proofs.as_mut() {
            Some((session, proofs)) => {
                crate::durable_knowledge::manifest::resource_status_with_proofs(
                    tx,
                    tenant,
                    workspace,
                    principal,
                    run.id,
                    run.scope_id,
                    run.slice_id,
                    run.current_phase_id.as_deref(),
                    knowledge_resources.as_ref(),
                    *session,
                    proofs,
                )
                .await?
            }
            None => {
                crate::durable_knowledge::manifest::resource_status(
                    tx,
                    tenant,
                    workspace,
                    principal,
                    run.id,
                    run.scope_id,
                    run.slice_id,
                    run.current_phase_id.as_deref(),
                    knowledge_resources.as_ref(),
                )
                .await?
            }
        }
    } else {
        None
    };
    let (delivery_receipt, delivery_fresh) = if issue_delivery_receipt {
        let (receipt, fresh) = delivery_receipt::load_or_create_delivery_receipt(
            tx,
            tenant,
            workspace,
            run.id,
            run.revision,
            &run.definition_digest,
        )
        .await?;
        (Some(receipt), fresh)
    } else {
        (None, false)
    };
    let checkpoints = checkpoint::load_for_run(tx, tenant, workspace, run_id).await?;
    Ok(Some(PipelineRunContext {
        run,
        definition,
        inquiry,
        source_checkpoint,
        checkpoints,
        delivered_phases,
        attempts,
        bindings,
        outputs,
        outputs_complete: sqlx::query_scalar::<_,bool>("SELECT NOT EXISTS(SELECT 1 FROM slice_pipeline_phase_outputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND payload_erased)").bind(tenant).bind(workspace).bind(run_id).fetch_one(&mut **tx).await.map_err(storage_error)?,
        inputs,
        result,
        knowledge,
        knowledge_status,
        knowledge_resources,
        knowledge_resource_status,
        delivery_receipt,
        delivery_fresh,
    }))
    }).await
}

pub(crate) async fn load_existing_delivery_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
) -> Result<Option<PipelineDeliveryReceipt>> {
    let Some(context) =
        load_context_without_delivery_receipt(tx, tenant, workspace, principal, run_id).await?
    else {
        return Ok(None);
    };
    delivery_receipt::load_existing(
        tx,
        tenant,
        workspace,
        run_id,
        context.run.revision,
        &context.run.definition_digest,
    )
    .await
}

pub(crate) async fn load_output(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run_id: Uuid,
    output_id: Uuid,
    digest: &str,
) -> Result<Option<PipelinePhaseOutput>> {
    authorize_context_copies(tx, tenant, workspace, principal, run_id).await?;
    let erased:Option<bool>=sqlx::query_scalar("SELECT payload_erased FROM slice_pipeline_phase_outputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND id=$4")
        .bind(tenant).bind(workspace).bind(run_id).bind(output_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if erased == Some(true) {
        return Err(Error::KnowledgePayloadErased);
    }
    let row:Option<serde_json::Value>=sqlx::query_scalar(
        "SELECT pg_catalog.jsonb_build_object('id',o.id,'run_id',o.run_id,'phase_id',o.phase_id,'phase_ordinal',o.phase_ordinal,'revision',o.revision,'body',o.body,'producer_context_id',o.producer_context_id,'digest',o.body_digest,'reference',o.reference,'knowledge_publication',o.knowledge_publication,'fields',o.fields,'verdict',o.verdict,'dispositions',o.dispositions,'skill_reads',o.skill_reads,'resource_reads',o.resource_reads,'artifacts',o.artifacts,'evidence_artifacts',o.evidence_artifacts,'validator_receipts',o.validator_receipts,'followup_proposal',o.followup_proposal,'stale',COALESCE(b.stale,true),'stale_reason',CASE WHEN b.output_id IS NULL THEN 'not_current_binding' ELSE b.stale_reason END) FROM slice_pipeline_phase_outputs o LEFT JOIN slice_pipeline_output_bindings b ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 AND o.id=$4 AND o.body_digest=$5")
        .bind(tenant).bind(workspace).bind(run_id).bind(output_id).bind(digest)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    row.map(decode).transpose()
}
