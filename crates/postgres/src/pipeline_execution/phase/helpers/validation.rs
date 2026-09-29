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
