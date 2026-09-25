async fn start_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
    verification_current: Option<bool>,
    authorized_policy: Option<&AdvisoryBudgetPolicy>,
    monotonic_elapsed_ms: Option<i64>,
) -> Result<AdvisoryDispatchStart> {
    let mut row = dispatch_by_id(tx, tenant, workspace, dispatch_id, true).await?;
    let (should_send, budget_reservation) = match dispatch_state(&row.state)? {
        AdvisoryDispatchState::Authorized => {
            let opportunity =
                opportunity_by_id(tx, tenant, workspace, row.opportunity_id, true).await?;
            if !supported_dispatch_opportunity(&opportunity) {
                return Err(Error::InputConflict);
            }
            if verification_current.is_some()
                && opportunity.capability != AdvisoryCapability::EngineeringProfile
            {
                return Err(Error::InputConflict);
            }
            if opportunity.capability == AdvisoryCapability::EngineeringProfile {
                let expected_state = if row.attempt_number == 1
                    && retry_basis(&row.retry_basis)? == AdvisoryRetryBasis::Initial
                {
                    opportunity.state == AdvisoryOpportunityState::Prepared
                        && opportunity.primary_reason == AdvisoryReason::DispatchAuthorized
                } else {
                    matches!(
                        opportunity.state,
                        AdvisoryOpportunityState::Prepared | AdvisoryOpportunityState::Failed
                    )
                };
                if !expected_state || opportunity.material_digest != row.material_digest {
                    return Err(Error::InputConflict);
                }
                let current_config: (i64, String, Option<String>, Option<serde_json::Value>) =
                    sqlx::query_as(
                        "SELECT revision,mode,provider_profile_ref,model_configuration \
                         FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 \
                         FOR UPDATE",
                    )
                    .bind(tenant)
                    .bind(workspace)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(storage_error)?;
                let stale_reason = if verification_current == Some(false) {
                    Some(AdvisoryReason::MatrixVerificationStale)
                } else if current_config.0 != opportunity.config_revision
                    || current_config.1 != "optional"
                    || current_config.2.is_none()
                    || current_config.3.is_none()
                {
                    Some(AdvisoryReason::ConfigurationChanged)
                } else {
                    match require_current_matrix_choice(tx, tenant, workspace, &opportunity).await {
                        Ok(MatrixChoiceStatus::Current) => None,
                        Ok(MatrixChoiceStatus::VerificationStale) => {
                            Some(AdvisoryReason::DeterministicInputInvalid)
                        }
                        Err(Error::StaleContext) => Some(AdvisoryReason::DeterministicInputInvalid),
                        Err(error) => return Err(error),
                    }
                };
                if let Some(reason) = stale_reason {
                    terminalize_stale_matrix_dispatch(
                        tx,
                        tenant,
                        workspace,
                        &mut row,
                        &opportunity,
                        reason,
                    )
                    .await?;
                    return Ok(AdvisoryDispatchStart {
                        dispatch: dispatch_from_row(&row)?,
                        should_send: false,
                        budget_reservation: None,
                    });
                }
            }
            let authored_source =
                if opportunity.capability == AdvisoryCapability::ScopeDecomposition {
                    authored_scope_source(tx, tenant, workspace, row.opportunity_id, None).await?
                } else {
                    None
                };
            if let Some(source) = authored_source {
                if row.attempt_number != 1
                    || retry_basis(&row.retry_basis)? != AdvisoryRetryBasis::Initial
                    || opportunity.state != AdvisoryOpportunityState::Prepared
                    || opportunity.primary_reason != AdvisoryReason::DispatchAuthorized
                {
                    return Err(Error::InputConflict);
                }
                let current_config: (i64, String, Option<String>, Option<serde_json::Value>) =
                    sqlx::query_as(
                        "SELECT revision,mode,provider_profile_ref,model_configuration \
                         FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 \
                         FOR UPDATE",
                    )
                    .bind(tenant)
                    .bind(workspace)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(storage_error)?;
                let stale_reason = if current_config.0 != opportunity.config_revision
                    || current_config.1 != "optional"
                    || current_config.2.is_none()
                    || current_config.3.is_none()
                {
                    Some(AdvisoryReason::ConfigurationChanged)
                } else {
                    match require_current_authored_scope_source(tx, tenant, workspace, &source)
                        .await
                    {
                        Ok(()) => None,
                        Err(Error::StaleContext) => Some(AdvisoryReason::DeterministicInputInvalid),
                        Err(error) => return Err(error),
                    }
                };
                if let Some(reason) = stale_reason {
                    terminalize_stale_authored_dispatch(
                        tx,
                        tenant,
                        workspace,
                        &mut row,
                        &opportunity,
                        reason,
                    )
                    .await?;
                    return Ok(AdvisoryDispatchStart {
                        dispatch: dispatch_from_row(&row)?,
                        should_send: false,
                        budget_reservation: None,
                    });
                }
            }
            let budget_reservation = reserve_before_dispatch(tx, tenant, workspace, &row,
                authorized_policy,
                matches!(opportunity.capability, AdvisoryCapability::ScopeDecomposition | AdvisoryCapability::EngineeringProfile)
                    .then_some(&row.configuration_snapshot),
                monotonic_elapsed_ms).await?;
            let dispatch_update = sqlx::query("UPDATE advisory_dispatch SET state='sending',send_certainty='sent_unknown',send_started_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='authorized'")
                .bind(tenant).bind(workspace).bind(dispatch_id).execute(&mut **tx).await.map_err(storage_error)?;
            let opportunity_update = sqlx::query("UPDATE advisory_opportunity SET state='awaiting_response',primary_reason='send_unknown',updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state IN ('prepared','failed')")
                .bind(tenant).bind(workspace).bind(row.opportunity_id).execute(&mut **tx).await.map_err(storage_error)?;
            if dispatch_update.rows_affected() != 1 || opportunity_update.rows_affected() != 1 {
                return Err(Error::InputConflict);
            }
            row.state = "sending".into();
            row.send_certainty = "sent_unknown".into();
            (true, Some(budget_reservation))
        }
        AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed => {
            (false, reservation_for_dispatch(tx, tenant, workspace, dispatch_id).await?)
        },
        AdvisoryDispatchState::Cancelled => return Err(Error::InputConflict),
    };
    Ok(AdvisoryDispatchStart {
        dispatch: dispatch_from_row(&row)?,
        should_send,
        budget_reservation,
    })
}

#[cfg(test)]
pub(crate) async fn authorize_dispatch_for_test(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    expected_config_revision: i64,
    input: &AdvisoryDispatchAuthorization,
) -> Result<AdvisoryDispatch> {
    authorize_dispatch(tx, tenant, workspace, expected_config_revision, input).await
}

#[cfg(test)]
pub(crate) async fn start_dispatch_for_test(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<AdvisoryDispatchStart> {
    start_dispatch(tx, tenant, workspace, dispatch_id, None, None, None).await
}

async fn seal_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    seal: &AdvisoryDispatchSeal,
) -> Result<AdvisoryDispatch> {
    seal.validate()?;
    let existing = dispatch_by_id(tx, tenant, workspace, seal.dispatch_id, true).await?;
    match dispatch_state(&existing.state)? {
        AdvisoryDispatchState::Sending => {
            sqlx::query("UPDATE advisory_dispatch SET response_payload=$4,input_tokens=$5,output_tokens=$6,latency_ms=$7,state='sealed',send_certainty=$8,outcome=$9,raw_response_ref=$10,sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='sending'")
                .bind(tenant).bind(workspace).bind(seal.dispatch_id).bind(&seal.response_payload).bind(seal.input_tokens).bind(seal.output_tokens).bind(seal.latency_ms).bind(seal.send_certainty.as_str()).bind(seal.outcome.as_str()).bind(&seal.raw_response_ref).execute(&mut **tx).await.map_err(storage_error)?;
            let sealed = dispatch_by_id(tx, tenant, workspace, seal.dispatch_id, false).await?;
            dispatch_from_row(&sealed)
        }
        AdvisoryDispatchState::Sealed => {
            if !dispatch_matches_seal(&existing, seal)? {
                return Err(Error::InputConflict);
            }
            dispatch_from_row(&existing)
        }
        _ => Err(Error::InputConflict),
    }
}

async fn cancel_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<AdvisoryDispatchCancellation> {
    let mut row = dispatch_by_id(tx, tenant, workspace, dispatch_id, true).await?;
    let outcome = match dispatch_state(&row.state)? {
        AdvisoryDispatchState::Authorized => {
            sqlx::query("UPDATE advisory_dispatch SET state='cancelled',send_certainty='not_sent',sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='authorized'")
                .bind(tenant).bind(workspace).bind(dispatch_id).execute(&mut **tx).await.map_err(storage_error)?;
            row.state = "cancelled".into();
            row.send_certainty = "not_sent".into();
            AdvisoryCancellationOutcome::Cancelled
        }
        AdvisoryDispatchState::Cancelled => AdvisoryCancellationOutcome::Cancelled,
        AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed => {
            AdvisoryCancellationOutcome::DeliveryMayHaveOccurred
        }
    };
    Ok(AdvisoryDispatchCancellation {
        dispatch: dispatch_from_row(&row)?,
        outcome,
    })
}

async fn reconcile_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    evidence: &AdvisoryReconciliationEvidence,
) -> Result<AdvisoryDispatch> {
    evidence.validate()?;
    let mut row = dispatch_by_id(tx, tenant, workspace, evidence.dispatch_id(), true).await?;
    match evidence {
        AdvisoryReconciliationEvidence::Inconclusive { .. } => {
            if dispatch_state(&row.state)? != AdvisoryDispatchState::Sending
                || send_certainty(&row.send_certainty)? != AdvisorySendCertainty::SentUnknown
            {
                return Err(Error::InputConflict);
            }
        }
        AdvisoryReconciliationEvidence::ConfirmedSent(seal) => {
            if dispatch_state(&row.state)? == AdvisoryDispatchState::Sealed {
                if !dispatch_matches_seal(&row, seal)? {
                    return Err(Error::InputConflict);
                }
            } else if dispatch_state(&row.state)? == AdvisoryDispatchState::Sending {
                sqlx::query("UPDATE advisory_dispatch SET response_payload=$4,input_tokens=$5,output_tokens=$6,latency_ms=$7,state='sealed',send_certainty='sent',outcome=$8,raw_response_ref=$9,sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='sending' AND send_certainty='sent_unknown'")
                    .bind(tenant).bind(workspace).bind(seal.dispatch_id).bind(&seal.response_payload).bind(seal.input_tokens).bind(seal.output_tokens).bind(seal.latency_ms).bind(seal.outcome.as_str()).bind(&seal.raw_response_ref).execute(&mut **tx).await.map_err(storage_error)?;
                row = dispatch_by_id(tx, tenant, workspace, seal.dispatch_id, false).await?;
            } else {
                return Err(Error::InputConflict);
            }
        }
        AdvisoryReconciliationEvidence::ConfirmedNotSent { evidence_ref, .. } => {
            if dispatch_state(&row.state)? == AdvisoryDispatchState::Sealed {
                if send_certainty(&row.send_certainty)? != AdvisorySendCertainty::NotSent
                    || dispatch_outcome(row.outcome.clone())?
                        != Some(AdvisoryDispatchOutcome::ProviderFailure)
                    || row.response_payload.is_some()
                    || row.raw_response_ref.as_deref() != Some(evidence_ref)
                {
                    return Err(Error::InputConflict);
                }
            } else if dispatch_state(&row.state)? == AdvisoryDispatchState::Sending {
                sqlx::query("UPDATE advisory_dispatch SET state='sealed',send_certainty='not_sent',outcome='provider_failure',raw_response_ref=$4,sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='sending' AND send_certainty='sent_unknown'")
                    .bind(tenant).bind(workspace).bind(evidence.dispatch_id()).bind(evidence_ref).execute(&mut **tx).await.map_err(storage_error)?;
                row = dispatch_by_id(tx, tenant, workspace, evidence.dispatch_id(), false).await?;
            } else {
                return Err(Error::InputConflict);
            }
        }
    }
    dispatch_from_row(&row)
}

pub(crate) async fn finalize_opportunity(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity_id: Uuid,
    expected_config_revision: i64,
    dispatch: &AdvisoryDispatch,
    verification_stale: bool,
) -> Result<AdvisoryOpportunity> {
    if dispatch.opportunity_id != opportunity_id {
        return Err(Error::InputConflict);
    }
    let opportunity = opportunity_by_id(tx, tenant, workspace, opportunity_id, true).await?;
    let persisted = dispatch_by_id(tx, tenant, workspace, dispatch.id, true).await?;
    if persisted.opportunity_id != opportunity_id {
        return Err(Error::InputConflict);
    }
    if !supported_dispatch_opportunity(&opportunity)
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
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.dispatch_id=$3"
    ).bind(tenant).bind(workspace).bind(persisted.id)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
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
        (AdvisoryOpportunityState::Unresolved, AdvisoryReason::SendUnknown)
    } else if budget_exhausted {
        (AdvisoryOpportunityState::Failed, AdvisoryReason::BudgetExhaustedAfterResponse)
    } else if current.0 != expected_config_revision || current.1 != "optional" {
        (
            AdvisoryOpportunityState::Invalidated,
            AdvisoryReason::ConfigurationChanged,
        )
    } else if let Some(reason) = matrix_stale {
        (AdvisoryOpportunityState::Invalidated, reason)
    } else if persisted_outcome == Some(AdvisoryDispatchOutcome::ProviderResponse) {
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
