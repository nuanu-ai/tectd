const DISPATCH_COLUMNS: &str = "id,opportunity_id,predecessor_dispatch_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,input_tokens,output_tokens,latency_ms,state,send_certainty,outcome,retry_basis,raw_response_ref";

#[derive(sqlx::FromRow)]
struct AuthoredScopeSourceRow {
    candidate_set_id: Uuid,
    source_digest: String,
    aggregate_payload: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct CurrentScopeAuthorityRow {
    candidate_set_revision: i64,
    current_snapshot_id: Uuid,
    input_cursor: i64,
    candidate_latest_input: i64,
    program_id: Uuid,
    program_revision: i64,
    program_current_latest: i64,
    program_payload_erased: bool,
    snapshot_program_revision: i64,
    snapshot_program_latest: i64,
    planning_latest_input: i64,
    selected_sources_digest: String,
    method_revision: String,
    method_digest: String,
    registry_revision: String,
    registry_digest: String,
}

async fn authored_scope_source(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity_id: Uuid,
    expected_candidate_set_id: Option<Uuid>,
) -> Result<Option<tect_domain::FrozenScopeSource>> {
    let row: Option<AuthoredScopeSourceRow> = sqlx::query_as(
        "SELECT m.candidate_set_id,s.source_digest,s.aggregate_payload \
         FROM advisory_scope_manifest m \
         JOIN advisory_scope_source_snapshot s \
           USING(tenant_id,workspace_id,opportunity_id,candidate_set_id,source_digest) \
         JOIN advisory_opportunity o \
           ON (o.tenant_id,o.workspace_id,o.id)=(m.tenant_id,m.workspace_id,m.opportunity_id) \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.opportunity_id=$3 \
           AND m.authored_request_digest IS NOT NULL \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' \
           AND o.work_item_id=m.candidate_set_id \
           AND o.capability='scope_decomposition' \
           AND o.decision_point='scope.decomposition.before_selection'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if expected_candidate_set_id.is_some_and(|id| id != row.candidate_set_id) {
        return Err(Error::StorageUnavailable);
    }
    let source_value = row
        .aggregate_payload
        .get("source")
        .cloned()
        .ok_or(Error::StorageUnavailable)?;
    let source: tect_domain::FrozenScopeSource =
        serde_json::from_value(source_value).map_err(storage_error)?;
    source
        .validate(&tect_application::Sha256ScopeDigest)
        .map_err(|_| Error::StorageUnavailable)?;
    if source.candidate_set_id != row.candidate_set_id || source.digest != row.source_digest {
        return Err(Error::StorageUnavailable);
    }
    Ok(Some(source))
}

async fn require_current_authored_scope_source(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    source: &tect_domain::FrozenScopeSource,
) -> Result<()> {
    let authority: Option<CurrentScopeAuthorityRow> = sqlx::query_as(
        "SELECT c.revision AS candidate_set_revision,c.current_snapshot_id,c.input_cursor,\
                c.latest_input AS candidate_latest_input,c.program_id,\
                p.revision AS program_revision,p.latest_input AS program_current_latest,\
                p.payload_erased AS program_payload_erased,\
                s.program_revision AS snapshot_program_revision,\
                s.program_latest_input AS snapshot_program_latest,s.planning_latest_input,\
                s.selected_sources_digest,s.method_revision,s.method_digest,\
                s.registry_revision,s.registry_digest \
         FROM scope_candidate_sets c JOIN programs p \
           ON (p.tenant_id,p.workspace_id,p.id)=(c.tenant_id,c.workspace_id,c.program_id) \
         JOIN scope_candidate_snapshots s \
           ON (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)=\
              (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 \
         FOR UPDATE OF c,p",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(source.candidate_set_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(authority) = authority else {
        return Err(Error::StaleContext);
    };
    if authority.candidate_set_revision != source.candidate_set_revision
        || authority.current_snapshot_id != source.snapshot_id
        || authority.input_cursor != source.input_cursor
        || authority.candidate_latest_input != source.planning_latest_input
        || source.input_cursor != source.planning_latest_input
        || authority.program_id != source.program_id
        || authority.program_revision != source.program_revision
        || authority.program_current_latest != source.program_latest_input
        || authority.program_payload_erased
        || authority.snapshot_program_revision != source.program_revision
        || authority.snapshot_program_latest != source.program_latest_input
        || authority.planning_latest_input != source.planning_latest_input
        || authority.selected_sources_digest != source.selected_sources_digest
        || authority.method_revision != source.method_revision
        || authority.method_digest != source.method_digest
        || authority.registry_revision != source.registry_revision
        || authority.registry_digest != source.registry_digest
    {
        return Err(Error::StaleContext);
    }
    Ok(())
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
        return dispatch_from_row(&existing);
    }
    if opportunity.capability != AdvisoryCapability::ScopeDecomposition
        || opportunity.material_digest != input.material_digest
    {
        return Err(Error::InputConflict);
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

async fn start_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<AdvisoryDispatchStart> {
    let mut row = dispatch_by_id(tx, tenant, workspace, dispatch_id, true).await?;
    let should_send = match dispatch_state(&row.state)? {
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
                    return Ok(AdvisoryDispatchStart {
                        dispatch: dispatch_from_row(&row)?,
                        should_send: false,
                    });
                }
            }
            sqlx::query("UPDATE advisory_dispatch SET state='sending',send_certainty='sent_unknown',send_started_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='authorized'")
                .bind(tenant).bind(workspace).bind(dispatch_id).execute(&mut **tx).await.map_err(storage_error)?;
            sqlx::query("UPDATE advisory_opportunity SET state='awaiting_response',primary_reason='send_unknown',updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state IN ('prepared','failed')")
                .bind(tenant).bind(workspace).bind(row.opportunity_id).execute(&mut **tx).await.map_err(storage_error)?;
            row.state = "sending".into();
            row.send_certainty = "sent_unknown".into();
            true
        }
        AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed => false,
        AdvisoryDispatchState::Cancelled => return Err(Error::InputConflict),
    };
    Ok(AdvisoryDispatchStart {
        dispatch: dispatch_from_row(&row)?,
        should_send,
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

async fn finalize_opportunity(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity_id: Uuid,
    expected_config_revision: i64,
    dispatch: &AdvisoryDispatch,
) -> Result<AdvisoryOpportunity> {
    let current: (i64, String) = sqlx::query_as("SELECT revision,mode FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE")
        .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let opportunity = opportunity_by_id(tx, tenant, workspace, opportunity_id, true).await?;
    let (state, reason) = if current.0 != expected_config_revision || current.1 != "optional" {
        (
            AdvisoryOpportunityState::Invalidated,
            AdvisoryReason::ConfigurationChanged,
        )
    } else if dispatch.outcome == Some(AdvisoryDispatchOutcome::ProviderResponse) {
        (
            AdvisoryOpportunityState::Advised,
            AdvisoryReason::ProviderResponse,
        )
    } else if dispatch.send_certainty == AdvisorySendCertainty::SentUnknown {
        (
            AdvisoryOpportunityState::Unresolved,
            AdvisoryReason::SendUnknown,
        )
    } else {
        (
            AdvisoryOpportunityState::Failed,
            AdvisoryReason::ProviderFailure,
        )
    };
    sqlx::query("UPDATE advisory_opportunity SET state=$4,primary_reason=$5,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(opportunity_id).bind(state.as_str()).bind(reason.as_str()).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(AdvisoryOpportunity {
        state,
        primary_reason: reason,
        provider_called: true,
        ..opportunity
    })
}
