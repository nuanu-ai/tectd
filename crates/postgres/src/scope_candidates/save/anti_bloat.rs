/// Persist an already-checked anti-bloat result through the same saved-draft
/// CAS, context and receipt machinery as the native draft caller. The caller
/// supplies the exact prior material and source-bound snapshot/cursor.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_preserved_anti_bloat_draft(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    receipt: &AntiBloatApplyReceipt,
    snapshot_id: Uuid,
    input_cursor: i64,
    before: &tect_domain::ResolvedCandidateDraft,
    after: &tect_domain::ResolvedCandidateDraft,
    request_payload: serde_json::Value,
) -> Result<StoredCandidateContext> {
    let locked = lock_set(
        transaction,
        tenant_id,
        workspace_id,
        receipt.candidate_set_id,
    )
    .await?;
    validate_write(&locked, receipt.from_revision, snapshot_id, input_cursor)?;
    if locked.status != CandidateSetStatus::ReviewRequired
        || locked.input_cursor != input_cursor
        || receipt.to_revision
            != locked
                .revision
                .checked_add(1)
                .ok_or(Error::StorageUnavailable)?
    {
        return Err(Error::StaleRevision);
    }
    let previous = required_context(
        transaction,
        tenant_id,
        workspace_id,
        receipt.candidate_set_id,
    )
    .await?;
    if previous.context.candidate_set.revision != receipt.from_revision
        || previous.draft.as_ref() != Some(before)
        || after.boundary != previous.context.candidate_set.boundary
    {
        return Err(Error::InputConflict);
    }
    after.validate()?;
    sqlx::query(
        "INSERT INTO scope_candidate_drafts \
         (tenant_id,workspace_id,candidate_set_id,set_revision,payload) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(receipt.candidate_set_id)
    .bind(receipt.to_revision)
    .bind(serde_json::to_value(after).map_err(storage_error)?)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    update_set(
        transaction,
        tenant_id,
        workspace_id,
        receipt.candidate_set_id,
        receipt.to_revision,
        CandidateSetStatus::ReviewRequired,
        locked.input_cursor,
        locked.latest_input,
        None,
    )
    .await?;
    let stored = required_context(
        transaction,
        tenant_id,
        workspace_id,
        receipt.candidate_set_id,
    )
    .await?;
    if stored.context.candidate_set.revision != receipt.to_revision
        || stored.draft.as_ref() != Some(after)
    {
        return Err(Error::StorageUnavailable);
    }
    insert_receipt(
        transaction,
        tenant_id,
        workspace_id,
        receipt.candidate_set_id,
        "anti_bloat_narrow",
        receipt.caller_request_id,
        request_payload,
        receipt.to_revision,
        serde_json::to_value(receipt).map_err(storage_error)?,
    )
    .await?;
    Ok(stored)
}

