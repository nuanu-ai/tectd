pub(crate) async fn finalize_opportunity(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity_id: Uuid,
    expected_config_revision: i64,
    dispatch: &AdvisoryDispatch,
    verification_stale: bool,
) -> Result<AdvisoryOpportunity> {
    finalize_interpreted_advisory_response(
        tx,
        tenant,
        workspace,
        opportunity_id,
        expected_config_revision,
        dispatch,
        verification_stale,
        true,
    )
    .await
}

// Preserve the generic dispatch contract while adding Matrix parse validity.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_interpreted_advisory_response(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity_id: Uuid,
    expected_config_revision: i64,
    dispatch: &AdvisoryDispatch,
    verification_stale: bool,
    provider_response_valid: bool,
) -> Result<AdvisoryOpportunity> {
    if dispatch.opportunity_id != opportunity_id {
        return Err(Error::InputConflict);
    }
    let opportunity = opportunity_by_id(tx, tenant, workspace, opportunity_id, true).await?;
    let persisted = dispatch_by_id(tx, tenant, workspace, dispatch.id, true).await?;
    if persisted.opportunity_id != opportunity_id {
        return Err(Error::InputConflict);
    }
    if opportunity.capability == AdvisoryCapability::PipelineRecommendation && provider_response_valid {
        let interpreted: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pipeline_advice_interpretations i WHERE i.tenant_id=$1 AND i.workspace_id=$2 AND i.opportunity_id=$3 AND i.dispatch_id=$4 AND i.manifest_digest=$5 AND i.contract_version=1 AND i.response_sha256=encode(sha256($6::bytea),'hex'))")
            .bind(tenant).bind(workspace).bind(opportunity_id).bind(persisted.id)
            .bind(&opportunity.material_digest).bind(&persisted.response_payload)
            .fetch_one(&mut **tx).await.map_err(storage_error)?;
        if !interpreted { return Err(Error::InputConflict); }
    }
    if !(supported_dispatch_opportunity(&opportunity)
        || (opportunity.capability == AdvisoryCapability::PipelineRecommendation
            && opportunity.decision_point == AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen))
        || expected_config_revision != opportunity.config_revision
        || persisted.material_digest != opportunity.material_digest
    {
        return Err(Error::InputConflict);
    }
    let latest_dispatch: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 \
         AND opportunity_id=$3 ORDER BY attempt_number DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if latest_dispatch != Some(persisted.id) {
        return Err(Error::InputConflict);
    }
    let persisted_state = dispatch_state(&persisted.state)?;
    let persisted_certainty = send_certainty(&persisted.send_certainty)?;
    let persisted_outcome = dispatch_outcome(persisted.outcome)?;
    if !matches!(
        (persisted_state, persisted_certainty, persisted_outcome),
        (
            AdvisoryDispatchState::Sealed,
            AdvisorySendCertainty::Sent,
            Some(
                AdvisoryDispatchOutcome::ProviderResponse
                    | AdvisoryDispatchOutcome::ProviderFailure
            )
        ) | (
            AdvisoryDispatchState::Sealed,
            AdvisorySendCertainty::NotSent | AdvisorySendCertainty::SentUnknown,
            Some(AdvisoryDispatchOutcome::ProviderFailure)
        ) | (
            AdvisoryDispatchState::Sending,
            AdvisorySendCertainty::SentUnknown,
            None
        )
    ) {
        return Err(Error::InputConflict);
    }
    let budget_row: Option<(Uuid, Option<bool>)> = sqlx::query_as(
        "SELECT r.dispatch_id,c.exhausted_after_response \
         FROM advisory_budget_reservations r LEFT JOIN advisory_budget_consumptions c \
         ON (c.tenant_id,c.workspace_id,c.dispatch_id)=(r.tenant_id,r.workspace_id,r.dispatch_id) \
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.dispatch_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(persisted.id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let budget_exhausted = match budget_row {
        None => false, // Historical pre-budget dispatches have no reservation.
        Some((_, Some(exhausted))) => exhausted,
        Some((_, None)) => return Err(Error::BudgetPolicyInvalid),
    };
    let current: (i64, String) = sqlx::query_as("SELECT revision,mode FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE")
        .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
    // Finalize Matrix advice against the same locked task head that guarded
    // persistence will use below. Otherwise a post-send task edit can make
    // persistence abort this transaction and strand the sealed dispatch.
    let matrix_stale = if verification_stale {
        if opportunity.capability != AdvisoryCapability::EngineeringProfile {
            return Err(Error::InputConflict);
        }
        Some(AdvisoryReason::MatrixVerificationStale)
    } else if opportunity.capability == AdvisoryCapability::EngineeringProfile
        && current.0 == expected_config_revision
        && current.1 == "optional"
    {
        match require_current_matrix_choice(tx, tenant, workspace, &opportunity).await {
            Ok(MatrixChoiceStatus::Current) => None,
            Ok(MatrixChoiceStatus::VerificationStale) => {
                Some(AdvisoryReason::MatrixVerificationStale)
            }
            Err(Error::StaleContext) => Some(AdvisoryReason::MatrixTaskRevisionChanged),
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let (state, reason) = if persisted_certainty == AdvisorySendCertainty::SentUnknown {
        (
            AdvisoryOpportunityState::Unresolved,
            AdvisoryReason::SendUnknown,
        )
    } else if budget_exhausted {
        (
            AdvisoryOpportunityState::Failed,
            AdvisoryReason::BudgetExhaustedAfterResponse,
        )
    } else if current.0 != expected_config_revision || current.1 != "optional" {
        (
            AdvisoryOpportunityState::Invalidated,
            AdvisoryReason::ConfigurationChanged,
        )
    } else if let Some(reason) = matrix_stale {
        (AdvisoryOpportunityState::Invalidated, reason)
    } else if provider_response_valid
        && persisted_outcome == Some(AdvisoryDispatchOutcome::ProviderResponse)
    {
        (
            AdvisoryOpportunityState::Advised,
            AdvisoryReason::ProviderResponse,
        )
    } else {
        (
            AdvisoryOpportunityState::Failed,
            AdvisoryReason::ProviderFailure,
        )
    };
    if opportunity.state == state && opportunity.primary_reason == reason {
        return Ok(finalized_opportunity(opportunity, state, reason));
    }
    if !matches!(
        opportunity.state,
        AdvisoryOpportunityState::AwaitingResponse | AdvisoryOpportunityState::Unresolved
    ) || !opportunity.state.can_transition_to(state)
    {
        return Err(Error::InputConflict);
    }
    let updated = sqlx::query("UPDATE advisory_opportunity SET state=$4,primary_reason=$5,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state=$6 AND primary_reason=$7")
        .bind(tenant).bind(workspace).bind(opportunity_id).bind(state.as_str()).bind(reason.as_str())
        .bind(opportunity.state.as_str()).bind(opportunity.primary_reason.as_str())
        .execute(&mut **tx).await.map_err(storage_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(finalized_opportunity(opportunity, state, reason))
}

// Matrix keeps its existing adapter contract over the shared finalization core.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_matrix_response(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity_id: Uuid,
    expected_config_revision: i64,
    dispatch: &AdvisoryDispatch,
    verification_stale: bool,
    provider_response_valid: bool,
) -> Result<AdvisoryOpportunity> {
    finalize_interpreted_advisory_response(
        tx,
        tenant,
        workspace,
        opportunity_id,
        expected_config_revision,
        dispatch,
        verification_stale,
        provider_response_valid,
    )
    .await
}
