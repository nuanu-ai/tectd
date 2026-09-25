use super::*;

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
    let request_bytes: Vec<u8> = row.try_get("request_payload").map_err(storage_error)?;
    let request: ModelRouteRankingWireRequest =
        serde_json::from_slice(&request_bytes).map_err(|_| Error::InputConflict)?;
    let attempted = ModelRoutePreparedAttempt {
        request,
        request_sha256: row.try_get("request_sha256").map_err(storage_error)?,
        request_bytes,
    };
    let raw_response: Vec<u8> = row.try_get("response_payload").map_err(storage_error)?;
    let response_sha256: String = row.try_get("response_sha256").map_err(storage_error)?;
    let outcome = parse_model_route_ranking_response(&attempted.request, &raw_response)?;
    let saved: Value = row.try_get("parsed_outcome").map_err(storage_error)?;
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
    evidence.verify(&prepared)?;
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
