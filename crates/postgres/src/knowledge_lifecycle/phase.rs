use super::*;

mod guard;
use guard::{replay_outcome, seal_gate, verify_envelope};

fn next_phase(
    definition: &KnowledgeChangeDefinition,
    current: KnowledgeChangePhaseId,
) -> Option<&KnowledgeChangePhaseDefinition> {
    definition
        .phases
        .iter()
        .find(|value| value.ordinal == current.ordinal() + 1)
}

fn requested_revisit(
    definition: &KnowledgeChangeDefinition,
    phase_id: KnowledgeChangePhaseId,
    output: &KnowledgeAgentPhaseOutputDraft,
    requested: Option<KnowledgeChangePhaseId>,
) -> Result<Option<KnowledgeChangePhaseId>> {
    if phase_id == KnowledgeChangePhaseId::KcResultHandoff {
        return if output.outcome == PipelinePhaseOutcome::Completed
            && output.transition == PipelineTransition::Complete
            && requested.is_none()
        {
            Ok(None)
        } else {
            Err(Error::InvalidArguments)
        };
    }
    if phase_id == KnowledgeChangePhaseId::KcReviewReconcile {
        let KnowledgeAgentPhaseData::KcReviewReconcile(review) = &output.data else {
            return Err(Error::InvalidArguments);
        };
        match review.outcome {
            KnowledgeReviewOutcome::Ready
            | KnowledgeReviewOutcome::NoChange
            | KnowledgeReviewOutcome::Rejected
                if output.outcome == PipelinePhaseOutcome::Completed
                    && output.transition == PipelineTransition::Continue
                    && requested.is_none() =>
            {
                return Ok(None);
            }
            KnowledgeReviewOutcome::Findings
                if output.outcome == PipelinePhaseOutcome::WaitingInput
                    && output.transition == PipelineTransition::Block => {}
            _ => return Err(Error::InvalidArguments),
        }
    } else if output.outcome == PipelinePhaseOutcome::Completed
        && output.transition == PipelineTransition::Continue
        && requested.is_none()
    {
        return Ok(None);
    } else if !matches!(
        output.outcome,
        PipelinePhaseOutcome::WaitingInput | PipelinePhaseOutcome::Blocked
    ) || !matches!(
        output.transition,
        PipelineTransition::Block | PipelineTransition::Escalate
    ) || output.findings.is_empty() && output.dispositions.is_empty()
    {
        return Err(Error::InvalidArguments);
    }
    if output.transition == PipelineTransition::Escalate {
        return if requested.is_none() {
            Ok(None)
        } else {
            Err(Error::InvalidArguments)
        };
    }
    let target = requested.ok_or(Error::InvalidArguments)?;
    let current = definition
        .phases
        .iter()
        .find(|value| value.id == phase_id)
        .ok_or(Error::InvalidConfiguration)?;
    if target != phase_id && !current.allowed_backward_to.contains(&target) {
        return Err(Error::InvalidArguments);
    }
    Ok(Some(target))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn complete_phase(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &CompleteKnowledgeChangePhase,
) -> Result<KnowledgeChangeMutationOutcome> {
    require_owner(tx, principal).await?;
    let payload = json(request)?;
    if let Some(prior) = replay::<KnowledgeChangeMutationOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "phase_complete",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    let generation = lock_workspace(tx, tenant, workspace).await?;
    let (revision, status, current) =
        lock_run(tx, tenant, workspace, request.change_id, request.run_id).await?;
    if let Some(prior) = replay::<KnowledgeChangeMutationOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "phase_complete",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    if revision != request.run_revision {
        return Err(Error::StaleRevision);
    }
    if !matches!(status.as_str(), "active" | "waiting_input" | "blocked")
        || current.as_deref() != Some(request.phase_id.as_str())
    {
        return Err(Error::StaleContext);
    }
    if matches!(
        request.phase_id,
        KnowledgeChangePhaseId::KcCommit | KnowledgeChangePhaseId::KcSettleEffects
    ) {
        return Err(Error::InvalidArguments);
    }
    let definition:KnowledgeChangeDefinition=decode(sqlx::query_scalar("SELECT definition FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).fetch_one(&mut **tx).await.map_err(storage_error)?)?;
    let (already_published,payload_erased,erased_no_change):(bool,bool,Option<serde_json::Value>)=sqlx::query_as("SELECT publisher_receipt IS NOT NULL OR erased_publisher_receipt IS NOT NULL,payload_erased,erased_no_change_proof FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let erased_no_change: Option<KnowledgeErasedNoChangeProof> =
        erased_no_change.map(decode).transpose()?;
    if already_published && request.phase_id != KnowledgeChangePhaseId::KcResultHandoff {
        return Err(Error::Forbidden);
    }
    let mut updates = super::phase_data::PhaseUpdates::empty();
    let mut output_id = None;
    let mut output_digest = None;
    if let Some(mut output) = request.output.clone() {
        verify_envelope(
            tx,
            tenant,
            workspace,
            request.run_id,
            &definition,
            request.phase_id,
            &output,
        )
        .await?;
        let phasewise = output.phasewise_reason.is_some()
            || matches!(
                output.outcome,
                PipelinePhaseOutcome::WaitingInput | PipelinePhaseOutcome::Blocked
            )
            || matches!(
                &output.data,
                KnowledgeAgentPhaseData::KcReviewReconcile(review)
                    if review.outcome == KnowledgeReviewOutcome::Findings
            );
        if phasewise {
            sqlx::query(
                "UPDATE knowledge_change_runs SET delivery_mode='phasewise' \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND delivery_mode='whole'",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(request.run_id)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
        }
        let advancing = output.outcome == PipelinePhaseOutcome::Completed
            && output.transition == PipelineTransition::Continue;
        updates = super::phase_data::process_agent_data(
            tx,
            tenant,
            workspace,
            principal,
            request.change_id,
            request.run_id,
            &mut output,
            advancing,
        )
        .await?;
        let id = Uuid::new_v4();
        let revision:i64=sqlx::query_scalar("SELECT COALESCE(MAX(revision),0)+1 FROM knowledge_change_outputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4").bind(tenant).bind(workspace).bind(request.run_id).bind(enum_text(&request.phase_id)?).fetch_one(&mut **tx).await.map_err(storage_error)?;
        let value_digest = if erased_no_change.is_some() {
            sqlx::query("INSERT INTO knowledge_change_outputs(id,tenant_id,workspace_id,run_id,phase_id,phase_ordinal,revision,payload_erased) VALUES($1,$2,$3,$4,$5,$6,$7,true)").bind(id).bind(tenant).bind(workspace).bind(request.run_id).bind(enum_text(&request.phase_id)?).bind(request.phase_id.ordinal() as i32).bind(revision).execute(&mut **tx).await.map_err(storage_error)?;
            None
        } else {
            let digest = digest(&output)?;
            sqlx::query("INSERT INTO knowledge_change_outputs(id,tenant_id,workspace_id,run_id,phase_id,phase_ordinal,revision,digest,output) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(id).bind(tenant).bind(workspace).bind(request.run_id).bind(enum_text(&request.phase_id)?).bind(request.phase_id.ordinal() as i32).bind(revision).bind(&digest).bind(json(&output)?).execute(&mut **tx).await.map_err(storage_error)?;
            Some(digest)
        };
        sqlx::query("INSERT INTO knowledge_change_output_bindings(tenant_id,workspace_id,run_id,phase_id,phase_ordinal,output_id,output_revision) VALUES($1,$2,$3,$4,$5,$6,$7) ON CONFLICT(tenant_id,workspace_id,run_id,phase_id) DO UPDATE SET output_id=EXCLUDED.output_id,output_revision=EXCLUDED.output_revision,stale=false,stale_reason=NULL,updated_at=pg_catalog.clock_timestamp()").bind(tenant).bind(workspace).bind(request.run_id).bind(enum_text(&request.phase_id)?).bind(request.phase_id.ordinal() as i32).bind(id).bind(revision).execute(&mut **tx).await.map_err(storage_error)?;
        output_id = Some(id);
        output_digest = value_digest;
    } else if request.phase_id == KnowledgeChangePhaseId::KcPublicationGate {
        updates.ready = Some(
            seal_gate(
                tx,
                tenant,
                workspace,
                request.change_id,
                request.run_id,
                revision,
                generation,
            )
            .await?,
        );
    }
    let (outcome, transition) = request
        .output
        .as_ref()
        .map(|value| (value.outcome, value.transition))
        .unwrap_or((
            PipelinePhaseOutcome::Completed,
            PipelineTransition::Continue,
        ));
    let revisit = match request.output.as_ref() {
        Some(output) => requested_revisit(
            &definition,
            request.phase_id,
            output,
            request.revisit_phase_id,
        )?,
        None if request.phase_id == KnowledgeChangePhaseId::KcPublicationGate
            && request.revisit_phase_id.is_none() =>
        {
            None
        }
        None => return Err(Error::InvalidArguments),
    };
    let attempt:i64=sqlx::query_scalar("SELECT COALESCE(MAX(attempt),0)+1 FROM knowledge_change_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4").bind(tenant).bind(workspace).bind(request.run_id).bind(enum_text(&request.phase_id)?).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let attempt_id = Uuid::new_v4();
    sqlx::query("INSERT INTO knowledge_change_attempts(id,tenant_id,workspace_id,run_id,phase_id,phase_ordinal,attempt,outcome,transition,output_id,output_digest,actor_session_id,payload_erased) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)").bind(attempt_id).bind(tenant).bind(workspace).bind(request.run_id).bind(enum_text(&request.phase_id)?).bind(request.phase_id.ordinal() as i32).bind(attempt).bind(enum_text(&outcome)?).bind(enum_text(&transition)?).bind(output_id).bind(output_digest).bind(session).bind(erased_no_change.is_some()).execute(&mut **tx).await.map_err(storage_error)?;
    let review_outcome = request
        .output
        .as_ref()
        .and_then(|output| match &output.data {
            KnowledgeAgentPhaseData::KcReviewReconcile(value) => Some(value.outcome),
            _ => None,
        });
    let (new_status, next) = match (outcome, transition) {
        (PipelinePhaseOutcome::Completed, PipelineTransition::Continue)
            if matches!(
                review_outcome,
                Some(KnowledgeReviewOutcome::NoChange | KnowledgeReviewOutcome::Rejected)
            ) =>
        {
            ("active", Some(KnowledgeChangePhaseId::KcResultHandoff))
        }
        (PipelinePhaseOutcome::Completed, PipelineTransition::Continue) => (
            "active",
            next_phase(&definition, request.phase_id).map(|value| value.id),
        ),
        (PipelinePhaseOutcome::Completed, PipelineTransition::Complete)
            if request.phase_id == KnowledgeChangePhaseId::KcResultHandoff =>
        {
            ("completed", None)
        }
        (PipelinePhaseOutcome::WaitingInput, _) => ("waiting_input", revisit),
        (PipelinePhaseOutcome::Blocked, PipelineTransition::Escalate) => ("escalated", None),
        (PipelinePhaseOutcome::Blocked, _) => ("blocked", revisit),
        _ => return Err(Error::InvalidArguments),
    };
    if let Some(target) = revisit {
        sqlx::query("UPDATE knowledge_change_output_bindings SET stale=true,stale_reason='review_revisit',updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_ordinal >= $4")
            .bind(tenant).bind(workspace).bind(request.run_id).bind(target.ordinal() as i32)
            .execute(&mut **tx).await.map_err(storage_error)?;
        updates.ready = None;
        sqlx::query("UPDATE knowledge_change_runs SET ready_to_commit=NULL WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(request.run_id).execute(&mut **tx).await.map_err(storage_error)?;
    }
    if let Some(review) = review_outcome {
        let value = match review {
            KnowledgeReviewOutcome::Ready => Some("ready"),
            KnowledgeReviewOutcome::NoChange => Some("no_change"),
            KnowledgeReviewOutcome::Rejected => Some("rejected"),
            KnowledgeReviewOutcome::Findings => None,
        };
        sqlx::query("UPDATE knowledge_change_runs SET terminal_review_outcome=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(request.run_id).bind(value).execute(&mut **tx).await.map_err(storage_error)?;
    }
    let next_text = next.as_ref().map(enum_text).transpose()?;
    let semantic_result = if payload_erased {
        None
    } else {
        updates.result.as_ref().map(json).transpose()?
    };
    let erased_result = if payload_erased {
        updates
            .result
            .as_ref()
            .map(KnowledgeErasedResult::from_validated)
            .as_ref()
            .map(json)
            .transpose()?
    } else {
        None
    };
    sqlx::query("UPDATE knowledge_change_runs SET revision=revision+1,status=$4,current_phase_id=$5,current_phase_ordinal=$6,baseline=COALESCE($7,baseline),branch_plan=COALESCE($8,branch_plan),ready_to_commit=COALESCE($9,ready_to_commit),result=COALESCE($10,result),erased_result=COALESCE($11,erased_result),updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.run_id).bind(new_status).bind(next_text).bind(next.map(|value|value.ordinal() as i32)).bind(updates.baseline.as_ref().map(json).transpose()?).bind(updates.plan.as_ref().map(json).transpose()?).bind(updates.ready.as_ref().map(json).transpose()?).bind(semantic_result).bind(erased_result).execute(&mut **tx).await.map_err(storage_error)?;
    if new_status == "completed" {
        let result = updates.result.as_ref().ok_or(Error::InternalInvariant)?;
        super::promotion::complete(
            tx,
            tenant,
            workspace,
            session,
            request.change_id,
            request.run_id,
            attempt_id,
            request.request_id,
            &definition.version,
            &definition.digest,
            result,
        )
        .await?;
    }
    if let Some(output_id) = output_id {
        super::erase::register_knowledge_change_output_copies(
            tx,
            tenant,
            workspace,
            request.change_id,
            request.run_id,
            attempt_id,
            output_id,
            request.request_id,
        )
        .await?;
    }
    super::erase::reconcile_change_owned_copies(tx, tenant, workspace, request.change_id).await?;
    let context = load_context(tx, tenant, workspace, request.change_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let result = KnowledgeChangeMutationOutcome::Advanced(Box::new(context));
    if erased_no_change.is_some() {
        sqlx::query(
            "INSERT INTO knowledge_lifecycle_command_receipts \
             (tenant_id,workspace_id,operation,request_id,actor_principal_id,actor_session_id, \
              payload_erased,erased_change_id) VALUES($1,$2,'phase_complete',$3,$4,$5,true,$6)",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(request.request_id)
        .bind(principal)
        .bind(session)
        .bind(request.change_id)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    } else {
        save_receipt(
            tx,
            tenant,
            workspace,
            principal,
            session,
            "phase_complete",
            request.request_id,
            &payload,
            &result,
        )
        .await?;
    }
    let erased: Option<KnowledgeErasedPublisherReceipt> =
        sqlx::query_scalar::<_, Option<serde_json::Value>>(
            "SELECT erased_publisher_receipt FROM knowledge_change_runs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(request.run_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?
        .map(decode)
        .transpose()?;
    let mut erased_units = erased
        .into_iter()
        .flat_map(|receipt| receipt.operations)
        .filter_map(|operation| match operation {
            KnowledgeRetainedOperationReceipt::PayloadErased(value) => Some(value.unit_id),
            KnowledgeRetainedOperationReceipt::Intact(_) => None,
        })
        .collect::<Vec<_>>();
    erased_units.extend(
        erased_no_change
            .iter()
            .flat_map(|proof| proof.operations.iter().map(|operation| operation.unit_id)),
    );
    if erased_units.is_empty() {
        return Ok(result);
    }
    super::erase::reconcile_change_owned_copies(tx, tenant, workspace, request.change_id).await?;
    erased_units.sort_unstable();
    erased_units.dedup();
    for unit in erased_units {
        let report = super::erase::suppress_owned_unit(tx, tenant, workspace, unit).await?;
        if report.remaining != 0 {
            return Err(Error::KnowledgeUnavailable);
        }
    }
    let sanitized = load_context(tx, tenant, workspace, request.change_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    Ok(KnowledgeChangeMutationOutcome::Advanced(Box::new(
        sanitized,
    )))
}
