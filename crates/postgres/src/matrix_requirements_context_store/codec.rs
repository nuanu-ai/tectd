use super::*;

pub(super) fn json<T: serde::Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(storage_error)
}
pub(super) fn signed(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::InvalidArguments)
}
pub(super) fn decode_proposal(value: Value) -> Result<MatrixRequirementsProposal> {
    let proposal: MatrixRequirementsProposal =
        serde_json::from_value(value.clone()).map_err(|_| Error::InputConflict)?;
    if json(&proposal)? != value {
        return Err(Error::InputConflict);
    }
    let rebuilt = MatrixRequirementsProposal::new(
        proposal.anchor(),
        proposal.revision(),
        proposal.patches().to_vec(),
        proposal.recorder().clone(),
    )?;
    if rebuilt != proposal {
        return Err(Error::InputConflict);
    }
    signed(proposal.revision())?;
    Ok(proposal)
}
pub(super) fn decode_confirmation(
    value: Value,
    proposal: &MatrixRequirementsProposal,
) -> Result<MatrixRequirementsConfirmation> {
    let confirmation: MatrixRequirementsConfirmation =
        serde_json::from_value(value.clone()).map_err(|_| Error::InputConflict)?;
    if json(&confirmation)? != value {
        return Err(Error::InputConflict);
    }
    confirmation.validate_for(proposal)?;
    Ok(confirmation)
}
pub(super) fn proposal_record(
    row: sqlx::postgres::PgRow,
) -> Result<StoredMatrixRequirementsProposal> {
    Ok(StoredMatrixRequirementsProposal {
        request: decode_proposal_request(row.try_get("request_payload").map_err(storage_error)?)?,
        proposal: decode_proposal(row.try_get("proposal_payload").map_err(storage_error)?)?,
        recorded_at_epoch_seconds: row
            .try_get("recorded_at_epoch_seconds")
            .map_err(storage_error)?,
    })
}
pub(super) fn confirmation_record(
    row: sqlx::postgres::PgRow,
) -> Result<StoredMatrixRequirementsConfirmation> {
    let proposal = decode_proposal(row.try_get("proposal_payload").map_err(storage_error)?)?;
    Ok(StoredMatrixRequirementsConfirmation {
        request: decode_confirmation_request(
            row.try_get("request_payload").map_err(storage_error)?,
        )?,
        confirmation: decode_confirmation(
            row.try_get("confirmation_payload").map_err(storage_error)?,
            &proposal,
        )?,
        recorded_at_epoch_seconds: row
            .try_get("recorded_at_epoch_seconds")
            .map_err(storage_error)?,
    })
}
pub(super) async fn recorder_ids(
    uow: &mut PgUnitOfWork,
    recorder: &DeclarationRecorder,
) -> Result<(Uuid, Uuid)> {
    let principal = Uuid::parse_str(&recorder.principal).map_err(|_| Error::InvalidArguments)?;
    let session = Uuid::parse_str(&recorder.session).map_err(|_| Error::InvalidArguments)?;
    if principal != uow.principal_id()?
        || crate::durable_knowledge_store::session_principal(uow, session).await? != principal
    {
        return Err(Error::Forbidden);
    }
    Ok((principal, session))
}
pub(super) fn anchor_ids(anchor: RequirementsAnchor) -> (Option<Uuid>, Option<Uuid>, Option<Uuid>) {
    match anchor {
        RequirementsAnchor::Program { .. } => (None, None, None),
        RequirementsAnchor::Scope { scope_id, .. } => (Some(scope_id), None, None),
        RequirementsAnchor::Slice {
            scope_id,
            candidate_set_id,
            work_candidate_id,
            ..
        } => (
            Some(scope_id),
            Some(candidate_set_id),
            Some(work_candidate_id),
        ),
    }
}
pub(super) fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()) {
        Some(code) if code == "23505" || code == "23514" => Error::InputConflict,
        Some(code) if code == "42501" => Error::Forbidden,
        _ => storage_error(error),
    }
}
