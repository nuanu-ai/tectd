use super::*;

pub(crate) async fn replay(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &CandidateReceiptRequest,
) -> Result<Option<StoredCandidateContext>> {
    if let CandidateReceiptRequest::SaveDraft(value) = request {
        if value.selected_advisory.is_none() {
            value.draft.require_source_grounded()?;
        } else {
            // A selected payload only replays an exact immutable receipt here.
            // New writes still require the trusted selected caller boundary.
            value.draft.validate()?;
        }
    }
    let payload = match request {
        CandidateReceiptRequest::SaveDraft(value) => serde_json::to_value(value),
        CandidateReceiptRequest::Review(value) => serde_json::to_value(value),
        CandidateReceiptRequest::RecordInput(value) => serde_json::to_value(value),
        CandidateReceiptRequest::Refresh(value) => serde_json::to_value(value),
    }
    .map_err(storage_error)?;
    receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id(),
        request.operation(),
        request.request_id(),
        &payload,
    )
    .await?
    .map(serde_json::from_value)
    .transpose()
    .map_err(storage_error)
}
