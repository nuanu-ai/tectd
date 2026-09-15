use super::*;

pub(super) async fn validate_phase(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &CompletePipelinePhase,
    kind: &str,
    ordinal: u32,
) -> Result<()> {
    let kind = pipeline(kind)?;
    if !matches!(
        kind,
        PipelineKind::Research | PipelineKind::DeepBrainstorming
    ) {
        return Ok(());
    }
    let inquiry: serde_json::Value = sqlx::query_scalar(
        "SELECT inquiry FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND inquiry IS NOT NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::InternalInvariant)?;
    let inquiry: PipelineInquiryContract = decode(inquiry)?;
    inquiry.validate()?;
    match kind {
        PipelineKind::Research => {
            let PipelineInquiryCompletion::Research { allow_inconclusive } = inquiry.completion
            else {
                return Err(Error::InternalInvariant);
            };
            validate_research(
                tx,
                tenant,
                workspace,
                request,
                ordinal,
                inquiry.topic_level,
                allow_inconclusive,
            )
            .await
        }
        PipelineKind::DeepBrainstorming => {
            let PipelineInquiryCompletion::Decision { requested_outcome } = inquiry.completion
            else {
                return Err(Error::InternalInvariant);
            };
            validate_decision(
                tx,
                tenant,
                workspace,
                request,
                ordinal,
                inquiry.topic_level,
                requested_outcome,
            )
            .await
        }
        _ => unreachable!(),
    }
}

async fn validate_research(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &CompletePipelinePhase,
    ordinal: u32,
    topic: PipelineInquiryTopicLevel,
    allow_inconclusive: bool,
) -> Result<()> {
    let forward = request.outcome == PipelinePhaseOutcome::Completed
        && request.transition == PipelineTransition::Continue
        && request.revisit_phase_id.is_none();
    if ordinal == 1
        && forward
        && (field(request, "topic_level") != Some(enum_text(&topic)?.as_str())
            || field(request, "allow_inconclusive")
                != Some(if allow_inconclusive { "true" } else { "false" }))
    {
        return Err(Error::InvalidArguments);
    }
    if ordinal == 9
        && forward
        && field(request, "disposition") == Some("bounded_inconclusive")
        && !allow_inconclusive
    {
        return Err(Error::Forbidden);
    }
    if ordinal != 12 || request.transition != PipelineTransition::Complete {
        return Ok(());
    }
    let state = field(request, "result_state").ok_or(Error::InvalidArguments)?;
    if !matches!(state, "answered" | "negative_result" | "inconclusive") {
        return Err(Error::Forbidden);
    }
    if state == "inconclusive" {
        if !allow_inconclusive {
            return Err(Error::Forbidden);
        }
        require_prior_field(
            tx,
            tenant,
            workspace,
            request.run_id,
            9,
            "disposition",
            "bounded_inconclusive",
        )
        .await?;
    } else {
        require_prior_field(
            tx,
            tenant,
            workspace,
            request.run_id,
            9,
            "disposition",
            "ready",
        )
        .await?;
    }
    Ok(())
}

async fn validate_decision(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &CompletePipelinePhase,
    ordinal: u32,
    topic: PipelineInquiryTopicLevel,
    requested: PipelineDecisionOutcome,
) -> Result<()> {
    let forward = request.outcome == PipelinePhaseOutcome::Completed
        && request.transition == PipelineTransition::Continue
        && request.revisit_phase_id.is_none();
    if ordinal == 1
        && forward
        && (field(request, "topic_level") != Some(enum_text(&topic)?.as_str())
            || field(request, "requested_outcome") != Some(enum_text(&requested)?.as_str()))
    {
        return Err(Error::InvalidArguments);
    }
    if ordinal == 8 {
        let disposition = field(request, "disposition");
        if disposition == Some("pending_decision")
            && (request.outcome != PipelinePhaseOutcome::WaitingInput
                || request.transition != PipelineTransition::Continue)
        {
            return Err(Error::Forbidden);
        }
        if forward
            && disposition == Some("recommended")
            && requested == PipelineDecisionOutcome::Decision
        {
            return Err(Error::Forbidden);
        }
    }
    if ordinal == 9 && forward {
        let disposition = field(request, "disposition").ok_or(Error::InvalidArguments)?;
        require_prior_field(
            tx,
            tenant,
            workspace,
            request.run_id,
            8,
            "disposition",
            disposition,
        )
        .await?;
    }
    if ordinal == 10 && request.transition == PipelineTransition::Complete {
        let state = field(request, "result_state").ok_or(Error::InvalidArguments)?;
        if !matches!(state, "selected" | "recommended" | "rejected")
            || state == "recommended" && requested == PipelineDecisionOutcome::Decision
        {
            return Err(Error::Forbidden);
        }
        for ordinal in [8, 9] {
            require_prior_field(
                tx,
                tenant,
                workspace,
                request.run_id,
                ordinal,
                "disposition",
                state,
            )
            .await?;
        }
    }
    Ok(())
}

fn field<'a>(request: &'a CompletePipelinePhase, key: &str) -> Option<&'a str> {
    request.output.fields.get(key).map(String::as_str)
}

async fn require_prior_field(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    ordinal: u32,
    key: &str,
    expected: &str,
) -> Result<()> {
    let actual: Option<String> = sqlx::query_scalar(
        "SELECT o.fields->>$5 FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_ordinal=$4 AND NOT b.stale AND NOT o.payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .bind(ordinal as i32)
    .bind(key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .flatten();
    if actual.as_deref() == Some(expected) {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}
