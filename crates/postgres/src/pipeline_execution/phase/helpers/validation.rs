use super::*;

pub(crate) async fn validate_consumed_outputs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    current_ordinal: u32,
    supplied: &[PipelineConsumedOutput],
) -> Result<()> {
    let rows:Vec<(String,i64,String)>=sqlx::query_as("SELECT b.phase_id,b.output_revision,o.body_digest FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_ordinal<$4 AND b.stale=false ORDER BY b.phase_ordinal")
        .bind(tenant).bind(workspace).bind(run).bind(current_ordinal as i32).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected = rows
        .into_iter()
        .map(|row| PipelineConsumedOutput {
            phase_id: row.0,
            output_revision: row.1,
            digest: row.2,
        })
        .collect::<Vec<_>>();
    if expected == supplied {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

pub(crate) async fn validate_consumed_inputs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase_id: &str,
    supplied: &[PipelineConsumedInput],
) -> Result<()> {
    let rows:Vec<(Uuid,i64,String)>=sqlx::query_as("SELECT id,sequence,input_digest FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4 ORDER BY sequence")
        .bind(tenant).bind(workspace).bind(run).bind(phase_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected = rows
        .into_iter()
        .map(|row| PipelineConsumedInput {
            input_id: row.0,
            sequence: row.1,
            digest: row.2,
        })
        .collect::<Vec<_>>();
    if expected == supplied {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

pub(crate) async fn validate_review_authorization(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let mut required = Vec::new();
    let mut specification_lineage: Option<(Vec<String>, String)> = None;
    for constraint in &phase.output_constraints {
        match constraint {
            PipelineOutputConstraint::EngineeringReview {
                stage,
                success_verdicts,
                required_prior_review_phase_ids,
                required_reconciliation_phase_id,
                ..
            } if output
                .verdict
                .as_deref()
                .is_some_and(|value| success_verdicts.iter().any(|success| success == value)) =>
            {
                required.extend(required_prior_review_phase_ids.iter().cloned());
                if stage == "specification"
                    && let Some(reconciliation_id) = required_reconciliation_phase_id
                {
                    specification_lineage = Some((
                        required_prior_review_phase_ids.clone(),
                        reconciliation_id.clone(),
                    ));
                }
            }
            PipelineOutputConstraint::CodeAuthorization {
                required_plan_review_phase_id,
            } => {
                required.push(required_plan_review_phase_id.clone());
            }
            _ => {}
        }
    }
    required.sort_unstable();
    required.dedup();
    for required_id in required {
        let prior = definition
            .phases
            .iter()
            .find(|candidate| candidate.id == required_id)
            .ok_or(Error::InternalInvariant)?;
        let accepted = prior
            .output_constraints
            .iter()
            .find_map(|constraint| match constraint {
                PipelineOutputConstraint::EngineeringReview {
                    success_verdicts, ..
                } => Some(success_verdicts),
                _ => None,
            })
            .ok_or(Error::InternalInvariant)?;
        let prior_verdict: Option<String> = sqlx::query_scalar(
            "SELECT o.verdict FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 AND b.stale=false AND NOT o.payload_erased",
        )
        .bind(tenant).bind(workspace).bind(run).bind(&required_id)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?
        .flatten();
        if prior_verdict
            .as_ref()
            .is_none_or(|value| !accepted.contains(value))
        {
            return Err(Error::Forbidden);
        }
    }
    if let Some((prior_phase_ids, reconciliation_phase_id)) = specification_lineage {
        engineering_findings::validate_specification_finding_lineage(
            tx,
            tenant,
            workspace,
            run,
            output,
            &prior_phase_ids,
            &reconciliation_phase_id,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn validate_reviewer_boundary(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase: &PipelinePhaseDefinition,
    request: &CompletePipelinePhase,
) -> Result<()> {
    if !phase.fresh_reviewer_input {
        return Ok(());
    }
    let attestation = request
        .output
        .reviewer_context
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let expected: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT o.producer_context_id FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_ordinal<$4 AND b.stale=false ORDER BY o.producer_context_id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .bind(phase.ordinal as i32)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut supplied = attestation.producer_context_ids.clone();
    supplied.sort_unstable();
    supplied.dedup();
    if attestation.reviewer_context_id != request.output.producer_context_id
        || expected != supplied
        || expected.contains(&request.output.producer_context_id)
        || supplied.len() != attestation.producer_context_ids.len()
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

/// Local-only admission is tied to the current backend-owned proof/policy pair.
/// It does not confer deployment authority or upgrade local proof to live proof.
pub(crate) async fn validate_local_result_gate(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &CompletePipelinePhase,
    definition: &PipelineDefinitionSnapshot,
) -> Result<()> {
    if definition.kind != PipelineKind::FullDesignToExecution
        || definition.version != NATIVE_WORK_CONTRACT_SUCCESSOR_VERSION
        || request.phase_id != "slice-deployment-or-handoff-gate"
        || request.output.verdict.as_deref() != Some("completed_local_verified")
    {
        return Ok(());
    }
    let mut prior = Vec::new();
    for phase_id in [
        "slice-plan-builder",
        "slice-execution-runner",
        "slice-verification-runner",
        "slice-validation-deployment-contract-shaper",
    ] {
        let row: Option<(i64, String, serde_json::Value, serde_json::Value, Option<String>, serde_json::Value)> = sqlx::query_as(
            "SELECT b.output_revision,o.body_digest,o.fields,o.dispositions,o.verdict,a.request_payload FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id JOIN slice_pipeline_phase_attempts a ON a.tenant_id=o.tenant_id AND a.workspace_id=o.workspace_id AND a.id=o.attempt_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 AND NOT b.stale AND NOT o.payload_erased AND NOT a.payload_erased AND a.outcome='completed' AND a.transition='continue' AND a.revisit_phase_id IS NULL",
        ).bind(tenant).bind(workspace).bind(request.run_id).bind(phase_id)
            .fetch_optional(&mut **tx).await.map_err(storage_error)?;
        let (revision, digest, fields, dispositions, verdict, payload) =
            row.ok_or_else(local_result_refusal)?;
        let attempted: CompletePipelinePhase = decode(payload)?;
        if attempted.run_id != request.run_id
            || attempted.phase_id != phase_id
            || attempted.output.fields != decode(fields)?
            || attempted.output.dispositions != decode::<Vec<String>>(dispositions)?
            || attempted.output.verdict != verdict
        {
            return Err(local_result_refusal());
        }
        prior.push((
            PipelineConsumedOutput {
                phase_id: phase_id.into(),
                output_revision: revision,
                digest,
            },
            attempted,
        ));
    }
    validate_local_execution(&prior[0], &prior[1], &prior[2])?;
    validate_local_result_pair(request, &prior[2], &prior[3])
}

fn local_result_refusal() -> Error {
    Error::refused_at(
        RefusalCode::InvalidOutput,
        "WP6-LOCAL-RESULT-01",
        "arguments.params.consumed_outputs",
        "current successful local verification and deployment-not-required policy consuming that exact verification",
        "local proof or policy is missing, stale, incomplete or contradictory",
        "refresh_current_local_proof_and_deployment_policy",
        "local_result_gate",
    )
}

pub(super) fn validate_local_result_pair(
    request: &CompletePipelinePhase,
    verification: &(PipelineConsumedOutput, CompletePipelinePhase),
    policy: &(PipelineConsumedOutput, CompletePipelinePhase),
) -> Result<()> {
    let verified = &verification.1;
    let shaped = &policy.1;
    let value = |output: &PipelinePhaseOutputDraft, key: &str, expected: &str| {
        output.fields.get(key).is_some_and(|v| v == expected)
    };
    let successful = |attempt: &CompletePipelinePhase| {
        attempt.outcome == PipelinePhaseOutcome::Completed
            && attempt.transition == PipelineTransition::Continue
            && attempt.revisit_phase_id.is_none()
    };
    if !successful(verified)
        || !successful(shaped)
        || verified.run_id != request.run_id
        || shaped.run_id != request.run_id
        || verification.0.phase_id != "slice-verification-runner"
        || policy.0.phase_id != "slice-validation-deployment-contract-shaper"
        || verified.output.verdict.as_deref() != Some("completed_local_verified")
        || verified.output.dispositions != ["proof_gate"]
        || !value(&verified.output, "verification_complete", "true")
        || !value(&verified.output, "failed_check_count", "0")
        || !value(&verified.output, "focused_exit_code", "0")
        || !value(&verified.output, "affected_exit_code", "0")
        || ["focused_command", "affected_command"].iter().any(|k| {
            verified
                .output
                .fields
                .get(*k)
                .is_none_or(|v| v.trim().is_empty() || v == "not_run")
        })
        || shaped.output.verdict.as_deref() != Some("deployment_not_required")
        || shaped.output.dispositions.is_empty()
        || shaped.output.dispositions.iter().any(|d| {
            !matches!(
                d.as_str(),
                "deployment_contract_gate" | "deployment_not_required"
            )
        })
        || !value(&shaped.output, "deployment_required", "false")
        || !value(&request.output, "deployment_required", "false")
        || !request.consumed_outputs.contains(&verification.0)
        || !request.consumed_outputs.contains(&policy.0)
        || !shaped.consumed_outputs.contains(&verification.0)
    {
        return Err(local_result_refusal());
    }
    Ok(())
}

pub(super) fn validate_local_execution(
    plan: &(PipelineConsumedOutput, CompletePipelinePhase),
    execution: &(PipelineConsumedOutput, CompletePipelinePhase),
    verification: &(PipelineConsumedOutput, CompletePipelinePhase),
) -> Result<()> {
    let attempt = &execution.1;
    let count = |key: &str| {
        attempt
            .output
            .fields
            .get(key)
            .and_then(|v| v.parse::<u64>().ok())
    };
    let expected = plan
        .1
        .output
        .fields
        .get("task_count")
        .and_then(|v| v.parse::<u64>().ok());
    if plan.0.phase_id != "slice-plan-builder"
        || plan.1.outcome != PipelinePhaseOutcome::Completed
        || plan.1.transition != PipelineTransition::Continue
        || plan.1.revisit_phase_id.is_some()
        || plan.1.output.verdict.as_deref() != Some("planned")
        || plan.1.output.dispositions != ["plan_gate"]
        || expected.is_none_or(|n| n == 0)
        || expected != count("task_count")
        || !attempt.consumed_outputs.contains(&plan.0)
        || execution.0.phase_id != "slice-execution-runner"
        || attempt.outcome != PipelinePhaseOutcome::Completed
        || attempt.transition != PipelineTransition::Continue
        || attempt.revisit_phase_id.is_some()
        || attempt.output.verdict.as_deref() != Some("implemented_locally")
        || count("task_count").is_none_or(|n| n == 0)
        || count("task_count") != count("completed_task_count")
        || count("blocked_task_count") != Some(0)
        || attempt
            .output
            .fields
            .get("reviews_complete")
            .map(String::as_str)
            != Some("true")
        || attempt
            .output
            .fields
            .get("no_hidden_lifecycle")
            .map(String::as_str)
            != Some("true")
        || !verification.1.consumed_outputs.contains(&execution.0)
    {
        return Err(local_result_refusal());
    }
    Ok(())
}
