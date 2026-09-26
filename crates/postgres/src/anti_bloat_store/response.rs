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
) -> Result<AntiBloatProviderObservation> {
    validate_permit_material(uow, permit).await?;
    let saved = saved_sealed_response(uow, permit.review_id)
        .await?
        .ok_or(Error::InputConflict)?;
    if saved.permit != *permit {
        return Err(Error::InputConflict);
    }
    Ok(saved.observation)
}

pub(super) async fn saved_sealed_response(
    uow: &mut PgUnitOfWork,
    review_id: Uuid,
) -> Result<Option<AntiBloatSealedResponse>> {
    let review = AntiBloatStore::review(uow, review_id)
        .await?
        .ok_or(Error::NotFound)?;
    if review.state != AntiBloatAttemptState::Sending {
        return Err(Error::InputConflict);
    }
    type Row = (
        Vec<u8>,
        String,
        String,
        Vec<u8>,
        String,
        Option<i32>,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        Option<bool>,
        Option<serde_json::Value>,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT request_bytes,request_sha256,request_adapter_identity,raw_response,response_sha256, \
         response_http_status,response_original_input_tokens,response_original_output_tokens,response_original_elapsed_ms,response_complete,original_transport_context \
         FROM public.scope_anti_bloat_reviews WHERE tenant_id=$1 AND review_id=$2 AND actor_id=$3 \
         AND state='sending' AND response_sealed_at IS NOT NULL AND raw_response IS NOT NULL",
    ).bind(uow.tenant_id()?).bind(review_id).bind(uow.principal_id()?)
        .fetch_optional(&mut **uow.transaction()?).await.map_err(storage_error)?;
    row.map(
        |(
            bytes,
            sha256,
            adapter_identity,
            raw,
            response_sha256,
            status,
            input_tokens,
            output_tokens,
            elapsed,
            complete,
            context,
        )| {
            if digest(&bytes) != sha256 || digest(&raw) != response_sha256 {
                return Err(Error::InputConflict);
            }
            Ok(AntiBloatSealedResponse {
                permit: AntiBloatSendPermit {
                    review_id,
                    request: AntiBloatPreparedRequest {
                        bytes,
                        sha256,
                        adapter_identity,
                        material_sha256: tect_application::anti_bloat_material_sha256(&review)?,
                    },
                },
                observation: AntiBloatProviderObservation {
                    response_complete: complete,
                    original_transport_context: context
                        .map(crate::advisory::decode_transport_context)
                        .transpose()?,
                    raw,
                    http_status: status
                        .map(u16::try_from)
                        .transpose()
                        .map_err(|_| Error::InputConflict)?,
                    input_tokens,
                    output_tokens,
                    elapsed_monotonic_ms: elapsed,
                },
            })
        },
    )
    .transpose()
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
