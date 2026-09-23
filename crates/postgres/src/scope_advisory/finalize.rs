async fn finalize_advice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &GuardedScopeAdviceRecord,
) -> Result<GuardedScopeAdvice> {
    let updated = sqlx::query(
        "UPDATE advisory_opportunity o SET state='advised',primary_reason='provider_response',updated_at=pg_catalog.clock_timestamp() \
         FROM advisory_workspace_config c,advisory_dispatch d \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3 \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' AND o.work_item_id=$4 \
           AND o.state='awaiting_response' AND o.config_revision=$5 \
           AND c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id \
           AND c.revision=$5 AND c.mode='optional' \
           AND d.tenant_id=o.tenant_id AND d.workspace_id=o.workspace_id \
           AND d.opportunity_id=o.id AND d.id=$6 AND d.material_digest=$7 \
           AND d.state='sealed' AND d.send_certainty='sent' AND d.outcome='provider_response'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.candidate_set_id)
    .bind(record.config_revision)
    .bind(record.dispatch_id)
    .bind(&record.dispatch_material_digest)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::StaleContext);
    }
    persist_advice(tx, tenant, workspace, record).await
}

async fn invalidate_advice_opportunity(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
) -> Result<()> {
    let updated = sqlx::query(
        "UPDATE advisory_opportunity SET state='invalidated',primary_reason='configuration_changed',updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='awaiting_response'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::StaleContext);
    }
    Ok(())
}

#[derive(sqlx::FromRow)]
struct PreparedScopeDispositionRow {
    state: String,
    primary_reason: String,
    source_digest: String,
}

fn prepared_scope_disposition_state(reason: AdvisoryReason) -> Result<&'static str> {
    let state = match reason {
        AdvisoryReason::DeterministicInputInvalid => AdvisoryOpportunityState::NoCall,
        AdvisoryReason::ConfigurationChanged => AdvisoryOpportunityState::Invalidated,
        _ => return Err(Error::InvalidArguments),
    };
    if !advisory_reason_matches_state(state, reason) {
        return Err(Error::InvalidArguments);
    }
    Ok(state.as_str())
}

fn valid_scope_source_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

async fn finish_prepared_scope_advisory_without_dispatch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &ScopePreparedAdvisoryDisposition,
) -> Result<()> {
    if !valid_scope_source_digest(&record.expected_source_digest) {
        return Err(Error::InvalidArguments);
    }
    let target_state = prepared_scope_disposition_state(record.reason)?;
    let row: PreparedScopeDispositionRow = sqlx::query_as(
        "SELECT o.state,o.primary_reason,m.source_digest \
         FROM advisory_opportunity o \
         JOIN advisory_scope_manifest m \
           ON (m.tenant_id,m.workspace_id,m.opportunity_id,m.candidate_set_id)=\
              (o.tenant_id,o.workspace_id,o.id,$4) \
         JOIN advisory_scope_source_snapshot s \
           ON (s.tenant_id,s.workspace_id,s.opportunity_id,s.candidate_set_id,s.source_digest)=\
              (m.tenant_id,m.workspace_id,m.opportunity_id,m.candidate_set_id,m.source_digest) \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3 \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' \
           AND o.work_item_id=$4 AND o.capability='scope_decomposition' \
           AND o.decision_point='scope.decomposition.before_selection' \
           AND o.source_revision=s.candidate_set_revision::text \
           AND o.config_revision=s.config_revision \
           AND o.material_digest=s.opportunity_material_digest \
         FOR UPDATE OF o",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.candidate_set_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    if row.source_digest != record.expected_source_digest {
        return Err(Error::InputConflict);
    }

    // Keep this as a separate statement after taking the opportunity row lock.
    // Dispatch authorization takes the same lock, so the fresh READ COMMITTED
    // snapshot sees an attempt committed while this call was waiting.
    let has_dispatch_attempt: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM advisory_dispatch \
         WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if has_dispatch_attempt {
        return Err(Error::InputConflict);
    }

    if row.state == target_state && row.primary_reason == record.reason.as_str() {
        return Ok(());
    }
    if row.state != AdvisoryOpportunityState::Prepared.as_str()
        || row.primary_reason != AdvisoryReason::DispatchAuthorized.as_str()
    {
        return Err(Error::StaleContext);
    }

    let updated = sqlx::query(
        "UPDATE advisory_opportunity o \
         SET state=$5,primary_reason=$6,updated_at=pg_catalog.clock_timestamp() \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3 \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' \
           AND o.work_item_id=$4 AND o.capability='scope_decomposition' \
           AND o.decision_point='scope.decomposition.before_selection' \
           AND o.state='prepared' AND o.primary_reason='dispatch_authorized' \
           AND EXISTS (SELECT 1 FROM advisory_scope_manifest m \
               JOIN advisory_scope_source_snapshot s \
                 ON (s.tenant_id,s.workspace_id,s.opportunity_id,s.candidate_set_id,s.source_digest)=\
                    (m.tenant_id,m.workspace_id,m.opportunity_id,m.candidate_set_id,m.source_digest) \
               WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.opportunity_id=$3 \
                 AND m.candidate_set_id=$4 AND m.source_digest=$7 \
                 AND s.config_revision=o.config_revision \
                 AND s.opportunity_material_digest=o.material_digest \
                 AND o.source_revision=s.candidate_set_revision::text) \
           AND NOT EXISTS (SELECT 1 FROM advisory_dispatch d \
               WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.opportunity_id=$3)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.candidate_set_id)
    .bind(target_state)
    .bind(record.reason.as_str())
    .bind(&record.expected_source_digest)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::StaleContext);
    }
    Ok(())
}
