async fn start_matrix_dispatch(
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
            if opportunity.capability != AdvisoryCapability::EngineeringProfile
                || !supported_dispatch_opportunity(&opportunity)
            {
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
            let budget_reservation = reserve_before_dispatch(
                tx,
                tenant,
                workspace,
                &row,
                authorized_policy,
                matches!(
                    opportunity.capability,
                    AdvisoryCapability::ScopeDecomposition | AdvisoryCapability::EngineeringProfile
                )
                .then_some(&row.configuration_snapshot),
                monotonic_elapsed_ms,
            )
            .await?;
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
        AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed => (
            false,
            reservation_for_dispatch(tx, tenant, workspace, dispatch_id).await?,
        ),
        AdvisoryDispatchState::Cancelled => return Err(Error::InputConflict),
    };
    Ok(AdvisoryDispatchStart {
        dispatch: dispatch_from_row(&row)?,
        should_send,
        budget_reservation,
    })
}

enum DispatchStartDisposition {
    Sendable,
    NonSend,
}

struct PreparedDispatchStart {
    row: DispatchRow,
    disposition: DispatchStartDisposition,
}

async fn prepare_dispatch_start(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<PreparedDispatchStart> {
    let mut row = dispatch_by_id(tx, tenant, workspace, dispatch_id, true).await?;
    let disposition = match dispatch_state(&row.state)? {
        AdvisoryDispatchState::Authorized => {
            let authored_source =
                authored_scope_source(tx, tenant, workspace, row.opportunity_id, None).await?;
            if let Some(source) = authored_source {
                let opportunity =
                    opportunity_by_id(tx, tenant, workspace, row.opportunity_id, true).await?;
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
                    return Ok(PreparedDispatchStart {
                        row,
                        disposition: DispatchStartDisposition::NonSend,
                    });
                }
            }
            DispatchStartDisposition::Sendable
        }
        AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed => {
            DispatchStartDisposition::NonSend
        }
        AdvisoryDispatchState::Cancelled => return Err(Error::InputConflict),
    };
    Ok(PreparedDispatchStart { row, disposition })
}

async fn finish_dispatch_start(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
    prepared: PreparedDispatchStart,
    budget_reservation: Option<AdvisoryBudgetReservation>,
) -> Result<AdvisoryDispatchStart> {
    let PreparedDispatchStart {
        mut row,
        disposition,
    } = prepared;
    let should_send = matches!(disposition, DispatchStartDisposition::Sendable);
    if should_send {
        sqlx::query("UPDATE advisory_dispatch SET state='sending',send_certainty='sent_unknown',send_started_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='authorized'")
            .bind(tenant).bind(workspace).bind(dispatch_id).execute(&mut **tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE advisory_opportunity SET state='awaiting_response',primary_reason='send_unknown',updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state IN ('prepared','failed')")
            .bind(tenant).bind(workspace).bind(row.opportunity_id).execute(&mut **tx).await.map_err(storage_error)?;
        row.state = "sending".into();
        row.send_certainty = "sent_unknown".into();
    }
    Ok(AdvisoryDispatchStart {
        budget_reservation,
        dispatch: dispatch_from_row(&row)?,
        should_send,
    })
}

async fn start_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<AdvisoryDispatchStart> {
    let prepared = prepare_dispatch_start(tx, tenant, workspace, dispatch_id).await?;
    finish_dispatch_start(tx, tenant, workspace, dispatch_id, prepared, None).await
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
    start_dispatch(tx, tenant, workspace, dispatch_id).await
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
            sqlx::query("UPDATE public.advisory_dispatch AS d SET response_payload=$4,pipeline_response_sha256=CASE WHEN EXISTS(SELECT 1 FROM public.advisory_opportunity AS o WHERE (o.tenant_id,o.workspace_id,o.id)=(d.tenant_id,d.workspace_id,d.opportunity_id) AND o.capability='pipeline_recommendation') THEN CASE WHEN $4::bytea IS NULL THEN NULL ELSE pg_catalog.encode(pg_catalog.sha256($4::bytea),'hex') END ELSE d.pipeline_response_sha256 END,input_tokens=$5,output_tokens=$6,latency_ms=$7,state='sealed',send_certainty=$8,outcome=$9,raw_response_ref=$10,sealed_at=pg_catalog.clock_timestamp() WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.id=$3 AND d.state='sending'")
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

fn signed_scope_dispatch_target(
    capability: AdvisoryCapability,
    decision_point: AdvisoryDecisionPoint,
    target_kind: &str,
    target_id: Option<Uuid>,
) -> Result<Uuid> {
    if capability != AdvisoryCapability::ScopeDecomposition
        || decision_point != AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection
        || target_kind != "scope_candidate_set"
    {
        return Err(Error::InputConflict);
    }
    target_id
        .filter(|id| !id.is_nil())
        .ok_or(Error::InputConflict)
}

async fn prepare_bound_scope_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<PreparedDispatchStart> {
    // Validate every branch, including non-send historical/replay states, while
    // retaining the same dispatch/opportunity locks through preparation.
    let row = dispatch_by_id(tx, tenant, workspace, dispatch_id, true).await?;
    let opportunity = opportunity_by_id(tx, tenant, workspace, row.opportunity_id, true).await?;
    let target = signed_scope_dispatch_target(
        opportunity.capability,
        opportunity.decision_point,
        &opportunity.target_kind,
        opportunity.target_id,
    )?;
    if row.id != dispatch_id
        || opportunity.id != row.opportunity_id
        || opportunity.workspace_id != workspace
        || opportunity.material_digest != row.material_digest
    {
        return Err(Error::InputConflict);
    }
    authored_scope_source(tx, tenant, workspace, opportunity.id, Some(target))
        .await?
        .ok_or(Error::InputConflict)?;
    prepare_dispatch_start(tx, tenant, workspace, dispatch_id).await
}

#[cfg(test)]
#[path = "signed_scope_start_tests.rs"]
mod signed_scope_start_tests;
