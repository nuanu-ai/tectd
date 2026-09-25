#[allow(clippy::too_many_arguments)]
pub(crate) async fn record_input(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    session_id: Uuid,
    request: &RecordCandidateInput,
    input_bytes: i64,
) -> Result<StoredCandidateContext> {
    let request_payload = serde_json::to_value(request).map_err(storage_error)?;
    if let Some(result) = receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        "record_input",
        request.request_id,
        &request_payload,
    )
    .await?
    {
        return serde_json::from_value(result).map_err(storage_error);
    }
    let locked = lock_set(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
    )
    .await?;
    if let Some(result) = receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        "record_input",
        request.request_id,
        &request_payload,
    )
    .await?
    {
        return serde_json::from_value(result).map_err(storage_error);
    }
    if locked.revision != request.revision {
        return Err(Error::StaleRevision);
    }
    let next_input = locked
        .latest_input
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    let next_revision = locked
        .revision
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    sqlx::query(
        "INSERT INTO scope_candidate_inputs \
             (tenant_id,workspace_id,candidate_set_id,sequence,request_id,session_id,input) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(request.candidate_set_id)
    .bind(next_input)
    .bind(request.request_id)
    .bind(session_id)
    .bind(&request.input)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "UPDATE scope_candidate_sets SET revision=$4,status='review_required',latest_input=$5,\
             max_input_bytes=GREATEST(max_input_bytes,$6) \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(request.candidate_set_id)
    .bind(next_revision)
    .bind(next_input)
    .bind(input_bytes)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let stored = required_context(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
    )
    .await?;
    insert_receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        "record_input",
        request.request_id,
        request_payload,
        next_revision,
        serde_json::to_value(&stored).map_err(storage_error)?,
    )
    .await?;
    Ok(stored)
}

pub(crate) async fn refresh(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &RefreshCandidateSet,
    material: &CandidateSnapshotMaterial,
) -> Result<StoredCandidateContext> {
    let request_payload = serde_json::to_value(request).map_err(storage_error)?;
    if let Some(result) = receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        "refresh",
        request.request_id,
        &request_payload,
    )
    .await?
    {
        return serde_json::from_value(result).map_err(storage_error);
    }
    let locked = lock_set(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
    )
    .await?;
    if let Some(result) = receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        "refresh",
        request.request_id,
        &request_payload,
    )
    .await?
    {
        return serde_json::from_value(result).map_err(storage_error);
    }
    if locked.revision != request.revision {
        return Err(Error::StaleRevision);
    }
    let snapshot_id = snapshot::insert(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        locked.latest_input,
        material,
    )
    .await?;
    let next_revision = locked
        .revision
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    let next_status = if required_context(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
    )
    .await?
    .draft
    .is_some()
    {
        CandidateSetStatus::ReviewRequired
    } else {
        CandidateSetStatus::Draft
    };
    update_set(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        next_revision,
        next_status,
        locked.input_cursor,
        locked.latest_input,
        Some(snapshot_id),
    )
    .await?;
    let stored = required_context(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
    )
    .await?;
    insert_receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        "refresh",
        request.request_id,
        request_payload,
        next_revision,
        serde_json::to_value(&stored).map_err(storage_error)?,
    )
    .await?;
    Ok(stored)
}
