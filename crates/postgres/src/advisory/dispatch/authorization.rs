const DISPATCH_COLUMNS: &str = "id,opportunity_id,predecessor_dispatch_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,input_tokens,output_tokens,latency_ms,state,send_certainty,outcome,retry_basis,raw_response_ref";

#[derive(PartialEq, Eq)]
enum MatrixChoiceStatus {
    Current,
    VerificationStale,
}

fn supported_dispatch_opportunity(opportunity: &AdvisoryOpportunity) -> bool {
    matches!(
        (opportunity.capability, opportunity.decision_point),
        (
            AdvisoryCapability::ScopeDecomposition,
            AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection
        ) | (
            AdvisoryCapability::EngineeringProfile,
            AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        )
    )
}

async fn require_current_matrix_choice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: &AdvisoryOpportunity,
) -> Result<MatrixChoiceStatus> {
    if opportunity.capability != AdvisoryCapability::EngineeringProfile
        || opportunity.decision_point != AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        || opportunity.workspace_id != workspace
        || opportunity.target_kind != "matrix_task"
        || opportunity.matrix_task_revision.is_none()
        || opportunity.matrix_task_revision != opportunity.work_revision
    {
        return Err(Error::InputConflict);
    }
    let task_id = opportunity.target_id.ok_or(Error::InputConflict)?;
    let expected_digest = opportunity
        .matrix_choice_set_digest
        .as_deref()
        .ok_or(Error::InputConflict)?;
    // The head lock serializes dispatch against an owner advancing the task.
    let current_revision: Option<i64> = sqlx::query_scalar(
        "SELECT current_revision FROM matrix_tasks \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if current_revision != opportunity.matrix_task_revision {
        return Err(Error::StaleContext);
    }
    let revision_binding: Option<(Option<String>, String)> = sqlx::query_as(
        "SELECT choice_set_digest,input_digest FROM matrix_task_revisions \
         WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND revision=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task_id)
    .bind(current_revision.ok_or(Error::StaleContext)?)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((choice_digest, input_digest)) = revision_binding else {
        return Err(Error::StaleContext);
    };
    if choice_digest.as_deref() != Some(expected_digest) {
        return Err(Error::StaleContext);
    }
    let Some(expected_verification) = opportunity.matrix_verification_digest.as_deref() else {
        return Ok(MatrixChoiceStatus::VerificationStale);
    };
    // The newest exact-revision verification is authoritative. Never fall back
    // to an older header if its replacement is stale or its evidence expired.
    let latest: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id,record_digest,input_digest FROM matrix_verifications \
         WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND task_revision=$4 \
         ORDER BY verified_at DESC,id DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(task_id)
    .bind(current_revision.ok_or(Error::StaleContext)?)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((verification_id, verification_digest, verified_input_digest)) = latest else {
        return Ok(MatrixChoiceStatus::VerificationStale);
    };
    if verification_digest != expected_verification || verified_input_digest != input_digest {
        return Ok(MatrixChoiceStatus::VerificationStale);
    }
    let bindings_valid: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM matrix_verification_bindings \
          WHERE tenant_id=$1 AND workspace_id=$2 AND verification_id=$3) \
         AND NOT EXISTS (SELECT 1 FROM matrix_verification_bindings \
          WHERE tenant_id=$1 AND workspace_id=$2 AND verification_id=$3 \
            AND (validation_outcome <> 'accepted' OR expires_at <= \
              EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())))",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(verification_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if !bindings_valid {
        return Ok(MatrixChoiceStatus::VerificationStale);
    }
    Ok(MatrixChoiceStatus::Current)
}

fn finalized_opportunity(
    opportunity: AdvisoryOpportunity,
    state: AdvisoryOpportunityState,
    reason: AdvisoryReason,
) -> AdvisoryOpportunity {
    AdvisoryOpportunity {
        state,
        primary_reason: reason,
        ..opportunity
    }
}

async fn terminalize_stale_authored_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: &mut DispatchRow,
    opportunity: &AdvisoryOpportunity,
    reason: AdvisoryReason,
) -> Result<()> {
    if row.attempt_number != 1
        || retry_basis(&row.retry_basis)? != AdvisoryRetryBasis::Initial
        || opportunity.state != AdvisoryOpportunityState::Prepared
        || opportunity.primary_reason != AdvisoryReason::DispatchAuthorized
    {
        return Err(Error::InputConflict);
    }
    let state = match reason {
        AdvisoryReason::DeterministicInputInvalid => "no_call",
        AdvisoryReason::ConfigurationChanged => "invalidated",
        _ => return Err(Error::InvalidArguments),
    };
    let canceled = sqlx::query(
        "UPDATE advisory_dispatch \
         SET state='cancelled',send_certainty='not_sent',sealed_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='authorized' \
           AND send_started_at IS NULL AND outcome IS NULL AND response_payload IS NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(row.id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if canceled.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    let terminalized = sqlx::query(
        "UPDATE advisory_opportunity \
         SET state=$4,primary_reason=$5,updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
           AND state='prepared' AND primary_reason='dispatch_authorized' \
           AND scope_id IS NULL AND work_item_kind='scope_candidate_set' \
           AND capability='scope_decomposition' \
           AND decision_point='scope.decomposition.before_selection'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity.id)
    .bind(state)
    .bind(reason.as_str())
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if terminalized.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    row.state = "cancelled".into();
    row.send_certainty = "not_sent".into();
    Ok(())
}

async fn terminalize_stale_matrix_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: &mut DispatchRow,
    opportunity: &AdvisoryOpportunity,
    reason: AdvisoryReason,
) -> Result<()> {
    let state = match reason {
        AdvisoryReason::DeterministicInputInvalid => "no_call",
        AdvisoryReason::ConfigurationChanged | AdvisoryReason::MatrixVerificationStale => {
            "invalidated"
        }
        _ => return Err(Error::InvalidArguments),
    };
    let cancelled = sqlx::query(
        "UPDATE advisory_dispatch \
         SET state='cancelled',send_certainty='not_sent',sealed_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='authorized' \
           AND send_started_at IS NULL AND outcome IS NULL AND response_payload IS NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(row.id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if cancelled.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    let terminalized = sqlx::query(
        "UPDATE advisory_opportunity \
         SET state=$4,primary_reason=$5,updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
           AND state IN ('prepared','failed') \
           AND capability='engineering_profile' \
           AND decision_point='engineering.profile.before_selection' \
           AND work_item_kind='matrix_task' \
           AND matrix_choice_set_digest IS NOT NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity.id)
    .bind(state)
    .bind(reason.as_str())
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if terminalized.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    row.state = "cancelled".into();
    row.send_certainty = "not_sent".into();
    Ok(())
}

async fn dispatch_by_id(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
    for_update: bool,
) -> Result<DispatchRow> {
    let suffix = if for_update { " FOR UPDATE" } else { "" };
    let sql = format!(
        "SELECT {DISPATCH_COLUMNS} FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3{suffix}"
    );
    sqlx::query_as(&sql)
        .bind(tenant)
        .bind(workspace)
        .bind(dispatch_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?
        .ok_or(Error::NotFound)
}

async fn authorize_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    expected_config_revision: i64,
    input: &AdvisoryDispatchAuthorization,
) -> Result<AdvisoryDispatch> {
    input.validate()?;
    let opportunity = opportunity_by_id(tx, tenant, workspace, input.opportunity_id, true).await?;
    if !supported_dispatch_opportunity(&opportunity) {
        return Err(Error::InputConflict);
    }
    if opportunity.capability == AdvisoryCapability::EngineeringProfile
        && (opportunity.state == AdvisoryOpportunityState::NoCall
            || opportunity.matrix_choice_set_digest.is_none())
    {
        return Err(Error::InputConflict);
    }
    let existing_sql = format!(
        "SELECT {DISPATCH_COLUMNS} FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 AND attempt_number=$4 FOR UPDATE"
    );
    let existing: Option<DispatchRow> = sqlx::query_as(&existing_sql)
        .bind(tenant)
        .bind(workspace)
        .bind(input.opportunity_id)
        .bind(input.attempt_number)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
    if let Some(existing) = existing {
        if !dispatch_matches_authorization(&existing, input)? {
            return Err(Error::InputConflict);
        }
        if opportunity.capability == AdvisoryCapability::EngineeringProfile {
            let current: (i64, String) = sqlx::query_as(
                "SELECT revision,mode FROM advisory_workspace_config \
                 WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
            )
            .bind(tenant)
            .bind(workspace)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
            if current.0 != expected_config_revision || current.0 != opportunity.config_revision {
                return Err(Error::StaleRevision);
            }
            if current.1 != "optional" {
                return Err(Error::InvalidConfiguration);
            }
            if require_current_matrix_choice(tx, tenant, workspace, &opportunity).await?
                != MatrixChoiceStatus::Current
            {
                return Err(Error::StaleContext);
            }
        }
        return dispatch_from_row(&existing);
    }
    if opportunity.material_digest != input.material_digest {
        return Err(Error::InputConflict);
    }
    if opportunity.primary_reason == AdvisoryReason::BudgetExhaustedAfterResponse {
        return Err(Error::BudgetExhaustedBeforeDispatch);
    }
    if input.retry_basis == AdvisoryRetryBasis::Initial {
        if opportunity.state != AdvisoryOpportunityState::Prepared {
            return Err(Error::InputConflict);
        }
    } else if !matches!(
        opportunity.state,
        AdvisoryOpportunityState::Prepared | AdvisoryOpportunityState::Failed
    ) {
        return Err(Error::InputConflict);
    }
    let current: (i64, String, Option<String>, Option<serde_json::Value>) = sqlx::query_as(
        "SELECT revision,mode,provider_profile_ref,model_configuration FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if current.0 != expected_config_revision || current.0 != opportunity.config_revision {
        return Err(Error::StaleRevision);
    }
    if current.1 != "optional" || current.2.is_none() || current.3.is_none() {
        return Err(Error::InvalidConfiguration);
    }
    if opportunity.capability == AdvisoryCapability::EngineeringProfile
        && require_current_matrix_choice(tx, tenant, workspace, &opportunity).await?
            != MatrixChoiceStatus::Current
    {
        return Err(Error::StaleContext);
    }
    if let Some(predecessor) = input.predecessor_dispatch_id {
        let previous = dispatch_by_id(tx, tenant, workspace, predecessor, true).await?;
        if previous.opportunity_id != input.opportunity_id
            || previous.attempt_number + 1 != input.attempt_number
            || previous.material_digest != input.material_digest
            || previous.payload_digest != input.payload_digest
            || !matches!(
                dispatch_state(&previous.state)?,
                AdvisoryDispatchState::Cancelled | AdvisoryDispatchState::Sealed
            )
            || !advisory_retry_permitted(
                send_certainty(&previous.send_certainty)?,
                input.retry_basis,
            )
        {
            return Err(Error::InputConflict);
        }
        let child_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 AND predecessor_dispatch_id=$4)",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(input.opportunity_id)
        .bind(predecessor)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
        if child_exists {
            return Err(Error::InputConflict);
        }
    }
    if let Some(source) = authored_scope_source(
        tx,
        tenant,
        workspace,
        input.opportunity_id,
        opportunity.target_id,
    )
    .await?
    {
        if input.attempt_number != 1
            || input.retry_basis != AdvisoryRetryBasis::Initial
            || opportunity.state != AdvisoryOpportunityState::Prepared
        {
            return Err(Error::InputConflict);
        }
        require_current_authored_scope_source(tx, tenant, workspace, &source).await?;
    }
    let inserted = sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,predecessor_dispatch_id,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'authorized','not_sent',$14) ON CONFLICT DO NOTHING")
        .bind(input.dispatch_id).bind(tenant).bind(workspace).bind(input.opportunity_id).bind(input.attempt_number).bind(input.predecessor_dispatch_id).bind(&input.provider).bind(&input.model).bind(&input.configuration_snapshot).bind(&input.configuration_digest).bind(&input.material_digest).bind(&input.payload_digest).bind(&input.request_payload).bind(input.retry_basis.as_str()).execute(&mut **tx).await.map_err(storage_error)?;
    let row: Option<DispatchRow> = sqlx::query_as(&existing_sql)
        .bind(tenant)
        .bind(workspace)
        .bind(input.opportunity_id)
        .bind(input.attempt_number)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
    let Some(row) = row else {
        return Err(Error::InputConflict);
    };
    if inserted.rows_affected() == 0 && !dispatch_matches_authorization(&row, input)? {
        return Err(Error::InputConflict);
    }
    if !dispatch_matches_authorization(&row, input)? {
        return Err(Error::InputConflict);
    }
    dispatch_from_row(&row)
}
