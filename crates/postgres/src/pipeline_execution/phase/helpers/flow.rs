pub(super) struct PlannedNextState {
    pub(super) status: &'static str,
    pub(super) next_id: Option<String>,
    pub(super) next_ordinal: Option<u32>,
    pub(super) revisit_ordinal: Option<u32>,
}

fn resolve_revisit<'a>(
    request: &CompletePipelinePhase,
    definition: &'a PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<Option<&'a PipelinePhaseDefinition>> {
    let Some(revisit) = &request.revisit_phase_id else {
        return Ok(None);
    };
    if !phase.allowed_backward_to.contains(revisit) {
        return Err(Error::Forbidden);
    }
    let target = definition
        .phases
        .iter()
        .find(|candidate| &candidate.id == revisit)
        .ok_or(Error::InvalidArguments)?;
    if target.ordinal >= phase.ordinal {
        return Err(Error::Forbidden);
    }
    Ok(Some(target))
}

pub(super) fn plan_next_state(
    request: &CompletePipelinePhase,
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<PlannedNextState> {
    if request.outcome == PipelinePhaseOutcome::WaitingInput {
        if request.transition != PipelineTransition::Continue {
            return Err(Error::InvalidArguments);
        }
        if let Some(target) = resolve_revisit(request, definition, phase)? {
            return Ok(PlannedNextState {
                status: "active",
                next_id: Some(target.id.clone()),
                next_ordinal: Some(target.ordinal),
                revisit_ordinal: Some(target.ordinal),
            });
        }
        return Ok(PlannedNextState {
            status: "waiting_input",
            next_id: Some(phase.id.clone()),
            next_ordinal: Some(phase.ordinal),
            revisit_ordinal: None,
        });
    }
    if request.outcome == PipelinePhaseOutcome::Blocked
        && request.transition == PipelineTransition::Continue
    {
        return Ok(PlannedNextState {
            status: "blocked",
            next_id: Some(phase.id.clone()),
            next_ordinal: Some(phase.ordinal),
            revisit_ordinal: None,
        });
    }
    match request.transition {
        PipelineTransition::Continue => {
            if request.outcome != PipelinePhaseOutcome::Completed {
                return Err(Error::InvalidArguments);
            }
            let revisit = resolve_revisit(request, definition, phase)?;
            let next = if let Some(revisit) = revisit {
                revisit
            } else {
                definition
                    .phases
                    .iter()
                    .find(|value| value.ordinal == phase.ordinal + 1)
                    .ok_or(Error::InvalidArguments)?
            };
            Ok(PlannedNextState {
                status: "active",
                next_id: Some(next.id.clone()),
                next_ordinal: Some(next.ordinal),
                revisit_ordinal: revisit.map(|_| next.ordinal),
            })
        }
        PipelineTransition::Complete => {
            if request.outcome != PipelinePhaseOutcome::Completed
                || phase.ordinal as usize != definition.phases.len()
            {
                return Err(Error::Forbidden);
            }
            Ok(PlannedNextState {
                status: "completed",
                next_id: None,
                next_ordinal: None,
                revisit_ordinal: None,
            })
        }
        PipelineTransition::Block => {
            if request.outcome != PipelinePhaseOutcome::Blocked {
                return Err(Error::InvalidArguments);
            }
            Ok(PlannedNextState {
                status: "blocked",
                next_id: Some(phase.id.clone()),
                next_ordinal: Some(phase.ordinal),
                revisit_ordinal: None,
            })
        }
        PipelineTransition::Escalate => {
            if request.outcome == PipelinePhaseOutcome::WaitingInput {
                return Err(Error::InvalidArguments);
            }
            Ok(PlannedNextState {
                status: "escalated",
                next_id: None,
                next_ordinal: None,
                revisit_ordinal: None,
            })
        }
    }
}

pub(super) async fn apply_rework(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    run: Uuid,
    plan: &PlannedNextState,
) -> Result<()> {
    let Some(target_ordinal) = plan.revisit_ordinal else {
        return Ok(());
    };
    let target_id = plan.next_id.as_deref().ok_or(Error::InternalInvariant)?;
    stale_from_ordinal(
        tx,
        tenant,
        workspace,
        session,
        run,
        target_ordinal,
        &format!("rework_from:{target_id}"),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn stale_from_ordinal(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    run: Uuid,
    target_ordinal: u32,
    reason: &str,
) -> Result<()> {
    sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=true,stale_reason=$4,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_ordinal>=$5")
        .bind(tenant).bind(workspace).bind(run).bind(reason).bind(target_ordinal as i32)
        .execute(&mut **tx).await.map_err(storage_error)?;
    checkpoint::mark_superseded_after_rework(tx, tenant, workspace, run, target_ordinal, session)
        .await
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
