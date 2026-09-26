use super::*;

pub(super) async fn record_preflight_no_call(
    uow: &mut PgUnitOfWork,
    review_id: Uuid,
    reason: AntiBloatNoCall,
) -> Result<()> {
    if !uow.is_read_write()
        || !matches!(
            reason,
            AntiBloatNoCall::ProviderUnconfigured
                | AntiBloatNoCall::PreflightInvalidConfiguration
                | AntiBloatNoCall::PreflightInvalidArguments
                | AntiBloatNoCall::PreflightInputConflict
                | AntiBloatNoCall::PreflightRequestTooLarge
        )
    {
        return Err(Error::InputConflict);
    }
    let changed = sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET state=$4 WHERE tenant_id=$1 \
        AND review_id=$2 AND actor_id=$3 AND state='prepared' AND request_bytes IS NULL \
        AND request_sha256 IS NULL AND send_started_at IS NULL AND raw_response IS NULL",
    )
    .bind(uow.tenant_id()?)
    .bind(review_id)
    .bind(uow.principal_id()?)
    .bind(state_name(&AntiBloatAttemptState::NoCall(reason)))
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if changed.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(())
}

pub(super) async fn validate_permit_material(
    uow: &mut PgUnitOfWork,
    permit: &AntiBloatSendPermit,
) -> Result<()> {
    let saved = AntiBloatStore::review(uow, permit.review_id)
        .await?
        .ok_or(Error::InputConflict)?;
    if saved.state != AntiBloatAttemptState::Sending
        || digest(&permit.request.bytes) != permit.request.sha256
        || tect_application::anti_bloat_material_sha256(&saved)? != permit.request.material_sha256
    {
        return Err(Error::InputConflict);
    }
    Ok(())
}

pub(super) async fn sealed_response_for_usage(
    uow: &mut PgUnitOfWork,
    permit: &AntiBloatSendPermit,
) -> Result<Vec<u8>> {
    validate_permit_material(uow, permit).await?;
    let raw: Option<Vec<u8>> = sqlx::query_scalar(
        "SELECT raw_response FROM scope_anti_bloat_reviews WHERE tenant_id=$1 \
         AND review_id=$2 AND actor_id=$3 AND state='sending' AND response_sealed_at IS NOT NULL \
         AND request_bytes=$4 AND request_sha256=$5 AND request_adapter_identity=$6",
    )
    .bind(uow.tenant_id()?)
    .bind(permit.review_id)
    .bind(uow.principal_id()?)
    .bind(&permit.request.bytes)
    .bind(&permit.request.sha256)
    .bind(&permit.request.adapter_identity)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    raw.ok_or(Error::InputConflict)
}

pub(super) async fn seal_terminal(
    uow: &mut PgUnitOfWork,
    permit: &AntiBloatSendPermit,
    state: AntiBloatAttemptState,
) -> Result<()> {
    if !uow.is_read_write()
        || !matches!(
            state,
            AntiBloatAttemptState::ProviderAbstained | AntiBloatAttemptState::InvalidResponse
        )
    {
        return Err(Error::InputConflict);
    }
    sealed_response_for_usage(uow, permit).await?;
    let changed = sqlx::query(
        "UPDATE scope_anti_bloat_reviews v SET state=$7,sealed_at=pg_catalog.clock_timestamp() \
         WHERE v.tenant_id=$1 AND v.review_id=$2 AND v.actor_id=$3 AND v.state='sending' \
         AND v.request_bytes=$4 AND v.request_sha256=$5 AND v.request_adapter_identity=$6 \
         AND v.raw_response IS NOT NULL AND v.response_sealed_at IS NOT NULL \
         AND EXISTS (SELECT 1 FROM scope_anti_bloat_budget_consumptions c WHERE \
             (c.tenant_id,c.workspace_id,c.review_id)=(v.tenant_id,v.workspace_id,v.review_id) \
             AND c.request_sha256=v.request_sha256 AND c.response_sha256=v.response_sha256 \
             AND NOT c.transport_failed AND ($7='invalid_response' OR \
                 (NOT c.unknown_usage AND NOT c.exhausted_after_response)))",
    )
    .bind(uow.tenant_id()?)
    .bind(permit.review_id)
    .bind(uow.principal_id()?)
    .bind(&permit.request.bytes)
    .bind(&permit.request.sha256)
    .bind(&permit.request.adapter_identity)
    .bind(state_name(&state))
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if changed.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(())
}
