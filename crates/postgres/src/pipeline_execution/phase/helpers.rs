use super::*;
mod engineering_findings;

pub(super) async fn validate_consumed_outputs(
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

pub(super) async fn validate_consumed_inputs(
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

pub(super) async fn validate_review_authorization(
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

pub(super) async fn validate_reviewer_boundary(
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

pub(super) async fn enforce_retry_policy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase: &PipelinePhaseDefinition,
) -> Result<()> {
    let attempts:i64=sqlx::query_scalar("SELECT COUNT(*) FROM slice_pipeline_phase_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4")
        .bind(tenant).bind(workspace).bind(run).bind(&phase.id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if attempts == 0 {
        return Ok(());
    }
    match phase.retry_policy {
        PipelinePhaseRetryPolicy::Repeatable => Ok(()),
        PipelinePhaseRetryPolicy::ExactReplayOnly => Err(Error::Forbidden),
        PipelinePhaseRetryPolicy::ReconciliationRequired => {
            let reconciled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM slice_pipeline_inputs i WHERE i.tenant_id=$1 AND i.workspace_id=$2 AND i.run_id=$3 AND i.phase_id=$4 AND i.created_at>(SELECT MAX(a.created_at) FROM slice_pipeline_phase_attempts a WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.run_id=$3 AND a.phase_id=$4))")
                .bind(tenant).bind(workspace).bind(run).bind(&phase.id).fetch_one(&mut **tx).await.map_err(storage_error)?;
            if reconciled {
                Ok(())
            } else {
                Err(Error::InputPending)
            }
        }
    }
}

pub(super) async fn next_state(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &CompletePipelinePhase,
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<(&'static str, Option<String>, Option<u32>)> {
    if request.outcome == PipelinePhaseOutcome::WaitingInput {
        if request.transition != PipelineTransition::Continue {
            return Err(Error::InvalidArguments);
        }
        return Ok(("waiting_input", Some(phase.id.clone()), Some(phase.ordinal)));
    }
    if request.outcome == PipelinePhaseOutcome::Blocked
        && request.transition == PipelineTransition::Continue
    {
        return Ok(("blocked", Some(phase.id.clone()), Some(phase.ordinal)));
    }
    match request.transition {
        PipelineTransition::Continue => {
            if request.outcome != PipelinePhaseOutcome::Completed {
                return Err(Error::InvalidArguments);
            }
            let next = if let Some(revisit) = &request.revisit_phase_id {
                if !phase.allowed_backward_to.contains(revisit) {
                    return Err(Error::Forbidden);
                }
                let target = definition
                    .phases
                    .iter()
                    .find(|value| &value.id == revisit)
                    .ok_or(Error::InvalidArguments)?;
                sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=true,stale_reason=$4,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_ordinal>=$5")
                    .bind(tenant).bind(workspace).bind(request.run_id).bind(format!("rework_from:{}",target.id)).bind(target.ordinal as i32)
                    .execute(&mut **tx).await.map_err(storage_error)?;
                checkpoint::mark_superseded_after_rework(
                    tx,
                    tenant,
                    workspace,
                    request.run_id,
                    target.ordinal,
                    session,
                )
                .await?;
                target
            } else {
                definition
                    .phases
                    .iter()
                    .find(|value| value.ordinal == phase.ordinal + 1)
                    .ok_or(Error::InvalidArguments)?
            };
            Ok(("active", Some(next.id.clone()), Some(next.ordinal)))
        }
        PipelineTransition::Complete => {
            if request.outcome != PipelinePhaseOutcome::Completed
                || phase.ordinal as usize != definition.phases.len()
            {
                return Err(Error::Forbidden);
            }
            Ok(("completed", None, None))
        }
        PipelineTransition::Block => {
            if request.outcome != PipelinePhaseOutcome::Blocked {
                return Err(Error::InvalidArguments);
            }
            Ok(("blocked", Some(phase.id.clone()), Some(phase.ordinal)))
        }
        PipelineTransition::Escalate => {
            if request.outcome == PipelinePhaseOutcome::WaitingInput {
                return Err(Error::InvalidArguments);
            }
            Ok(("escalated", None, None))
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn publish_result(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &CompletePipelinePhase,
    attempt_id: Uuid,
    run: &LockedRun,
    origin: &str,
    outcome: SliceResultOutcome,
) -> Result<SliceResult> {
    let draft = request
        .terminal_result
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let slice_revision:i64=sqlx::query_scalar("SELECT revision FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(run.1).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let revision:i64=sqlx::query_scalar("SELECT COALESCE(MAX(revision),0)+1 FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND slice_id=$3")
        .bind(tenant).bind(workspace).bind(run.1).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let id = Uuid::new_v4();
    let state = if outcome == SliceResultOutcome::Completed {
        "completed"
    } else {
        "blocked"
    };
    sqlx::query("INSERT INTO slice_results(id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,revision,outcome,summary,evidence,scope_impact,remaining_work,provenance,request_id,request_payload,pipeline_run_id,pipeline_definition_version,pipeline_definition_digest,pipeline_final_attempt_id,pipeline_result_origin) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'externally_reported',$13,$14,$15,$16,$17,$18,$19)")
        .bind(id).bind(tenant).bind(workspace).bind(run.0).bind(run.1).bind(slice_revision).bind(revision).bind(state)
        .bind(&draft.summary).bind(json(&draft.evidence)?).bind(&draft.scope_impact).bind(&draft.remaining_work).bind(request.request_id).bind(json(request)?)
        .bind(request.run_id).bind(&run.5).bind(&run.6).bind(attempt_id).bind(origin).execute(&mut **tx).await.map_err(storage_error)?;
    let next_slice_revision = slice_revision
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    sqlx::query("UPDATE native_slices SET revision=$4,state=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(run.1).bind(next_slice_revision).bind(state).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_pipeline_runs SET slice_revision=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(next_slice_revision).execute(&mut **tx).await.map_err(storage_error)?;
    let set_id:Uuid=sqlx::query_scalar("SELECT slice_candidate_set_id FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(run.0).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let set:(i64,i64)=sqlx::query_as("SELECT revision,latest_input FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(set_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let next_input = set.1.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query("INSERT INTO slice_planning_inputs(tenant_id,workspace_id,candidate_set_id,sequence,session_id,source_result_id,input) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(tenant).bind(workspace).bind(set_id).bind(next_input).bind(session).bind(id).bind(format!("Pipeline-managed Slice Result {id}: {}",draft.summary))
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_candidate_sets SET revision=revision+1,status='review_required',latest_input=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(set_id).bind(next_input).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE native_scopes SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(run.0).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(SliceResult {
        id,
        slice_id: run.1,
        slice_revision,
        revision,
        outcome,
        summary: draft.summary.clone(),
        evidence: draft.evidence.clone(),
        scope_impact: draft.scope_impact.clone(),
        remaining_work: draft.remaining_work.clone(),
        provenance: "externally_reported".into(),
        pipeline_run_id: Some(request.run_id),
        pipeline_definition_version: Some(run.5.clone()),
        pipeline_definition_digest: Some(run.6.clone()),
        pipeline_final_attempt_id: Some(attempt_id),
        pipeline_result_origin: Some(origin.into()),
        knowledge_provenance: None,
    })
}
