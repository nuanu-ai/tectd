use super::*;

pub(super) fn observation_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<ModelRouteProviderObservation>> {
    let Some(raw) = row
        .try_get::<Option<Vec<u8>>, _>("response_payload")
        .map_err(storage_error)?
    else {
        return Ok(None);
    };
    if row
        .try_get::<Option<String>, _>("response_sha256")
        .map_err(storage_error)?
        != Some(model_route_wire_sha256(&raw))
    {
        return Err(Error::InputConflict);
    }
    let status: Option<i32> = row.try_get("response_http_status").map_err(storage_error)?;
    Ok(Some(ModelRouteProviderObservation {
        response_complete: row.try_get("response_complete").map_err(storage_error)?,
        original_transport_context: row
            .try_get::<Option<Value>, _>("original_transport_context")
            .map_err(storage_error)?
            .map(crate::advisory::decode_transport_context)
            .transpose()?,
        raw,
        http_status: status
            .map(u16::try_from)
            .transpose()
            .map_err(|_| Error::InputConflict)?,
        input_tokens: row
            .try_get("response_original_input_tokens")
            .map_err(storage_error)?,
        output_tokens: row
            .try_get("response_original_output_tokens")
            .map_err(storage_error)?,
        elapsed_monotonic_ms: row
            .try_get("response_original_elapsed_ms")
            .map_err(storage_error)?,
    }))
}

pub(super) fn row_attempted(row: &sqlx::postgres::PgRow) -> Result<ModelRoutePreparedAttempt> {
    let request_bytes: Vec<u8> = row.try_get("request_payload").map_err(storage_error)?;
    let typed: Option<Value> = row
        .try_get("typed_request_payload")
        .map_err(storage_error)?;
    let request: ModelRouteRankingWireRequest = match typed {
        Some(value) => serde_json::from_value(value).map_err(|_| Error::InputConflict)?,
        None => serde_json::from_slice(&request_bytes).map_err(|_| Error::InputConflict)?,
    };
    Ok(ModelRoutePreparedAttempt {
        request,
        request_bytes,
        request_sha256: row.try_get("request_sha256").map_err(storage_error)?,
        adapter_identity: row.try_get("adapter_identity").map_err(storage_error)?,
    })
}

pub(super) fn row_permit(
    row: &sqlx::postgres::PgRow,
    workspace_id: Uuid,
    key: &str,
) -> Result<ModelRouteSendPermit> {
    Ok(ModelRouteSendPermit {
        attempt_id: row.try_get("id").map_err(storage_error)?,
        workspace_id,
        preparation_request_key: key.into(),
        request_sha256: row.try_get("request_sha256").map_err(storage_error)?,
        policy_id: row.try_get("policy_id").map_err(storage_error)?,
        policy_version: row.try_get("policy_version").map_err(storage_error)?,
        policy_digest: row.try_get("policy_digest").map_err(storage_error)?,
    })
}

pub(crate) async fn sealed_ranking(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    request_key: &str,
) -> Result<Option<ModelRouteSealedRankingEvidence>> {
    let prepared = uow
        .by_request(workspace_id, request_key)
        .await?
        .ok_or(Error::StaleContext)?;
    current_preparation(uow, &prepared).await?;
    let Some(row) = attempt_row(uow, workspace_id, request_key).await? else {
        return Ok(None);
    };
    if row.try_get::<String, _>("state").map_err(storage_error)? != "parsed" {
        return Ok(None);
    }
    let attempted = row_attempted(&row)?;
    let raw_response: Vec<u8> = row.try_get("response_payload").map_err(storage_error)?;
    let response_sha256: String = row.try_get("response_sha256").map_err(storage_error)?;
    let saved: Value = row.try_get("parsed_outcome").map_err(storage_error)?;
    let outcome = if attempted.adapter_identity.is_some() {
        serde_json::from_value(saved.clone()).map_err(|_| Error::InputConflict)?
    } else {
        parse_model_route_ranking_response(&attempted.request, &raw_response)?
    };
    if serde_json::to_value(&outcome).map_err(storage_error)? != saved {
        return Err(Error::InputConflict);
    }
    let evidence = ModelRouteSealedRankingEvidence {
        permit: row_permit(&row, workspace_id, request_key)?,
        attempted,
        raw_response,
        response_sha256,
        outcome,
    };
    evidence.validate_material(&prepared)?;
    Ok(Some(evidence))
}

pub(crate) async fn audit_state(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    request_key: &str,
) -> Result<Option<String>> {
    attempt_row(uow, workspace_id, request_key)
        .await?
        .map(|row| row.try_get("state").map_err(storage_error))
        .transpose()
}
