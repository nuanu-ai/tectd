use super::{resolve, snapshot, write::*};
use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{
    CandidateReceiptRequest, CandidateSetStatus, CandidateSnapshotMaterial, Error,
    RecordCandidateInput, RefreshCandidateSet, Result, ReviewCandidateSet, ReviewVerdict,
    SaveCandidateDraft, ScopeCandidateReview, StoredCandidateContext,
};
use uuid::Uuid;

pub(crate) async fn replay(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &CandidateReceiptRequest,
) -> Result<Option<StoredCandidateContext>> {
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

pub(crate) async fn save_draft(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &SaveCandidateDraft,
) -> Result<StoredCandidateContext> {
    save_draft_with_material(transaction, tenant_id, workspace_id, request, None).await
}

pub(crate) async fn save_draft_with_material(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &SaveCandidateDraft,
    selected_material: Option<&tect_domain::ResolvedCandidateDraft>,
) -> Result<StoredCandidateContext> {
    if request.selected_advisory.is_some() != selected_material.is_some() {
        return Err(Error::InvalidArguments);
    }
    let request_payload = serde_json::to_value(request).map_err(storage_error)?;
    if let Some(result) = receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        "save_draft",
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
        "save_draft",
        request.request_id,
        &request_payload,
    )
    .await?
    {
        return serde_json::from_value(result).map_err(storage_error);
    }
    validate_write(
        &locked,
        request.revision,
        request.snapshot_id,
        request.input_cursor,
    )?;
    if matches!(
        locked.status,
        CandidateSetStatus::Ready | CandidateSetStatus::Blocked
    ) {
        return Err(Error::Forbidden);
    }
    let previous = required_context(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
    )
    .await?;
    if request.draft.boundary != previous.context.candidate_set.boundary {
        return Err(Error::InvalidArguments);
    }
    let resolved = if let Some(material) = selected_material {
        material.clone()
    } else {
        resolve::resolve(
            transaction,
            &resolve::ResolveContext {
                tenant_id,
                workspace_id,
                candidate_set_id: request.candidate_set_id,
                snapshot_id: request.snapshot_id,
                latest_input: locked.latest_input,
            },
            &request.draft,
            previous.draft.as_ref(),
        )
        .await?
    };
    let next_revision = locked
        .revision
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    let draft_payload = serde_json::to_value(&resolved).map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO scope_candidate_drafts \
             (tenant_id,workspace_id,candidate_set_id,set_revision,payload) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(request.candidate_set_id)
    .bind(next_revision)
    .bind(&draft_payload)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    update_set(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        next_revision,
        CandidateSetStatus::ReviewRequired,
        request.input_cursor,
        locked.latest_input,
        None,
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
        "save_draft",
        request.request_id,
        request_payload,
        next_revision,
        serde_json::to_value(&stored).map_err(storage_error)?,
    )
    .await?;
    Ok(stored)
}

pub(crate) async fn save_review(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &ReviewCandidateSet,
) -> Result<StoredCandidateContext> {
    let request_payload = serde_json::to_value(request).map_err(storage_error)?;
    if let Some(result) = receipt(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        "save_review",
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
        "save_review",
        request.request_id,
        &request_payload,
    )
    .await?
    {
        return serde_json::from_value(result).map_err(storage_error);
    }
    validate_write(
        &locked,
        request.revision,
        request.snapshot_id,
        request.input_cursor,
    )?;
    if locked.status != CandidateSetStatus::ReviewRequired {
        return Err(Error::InvalidArguments);
    }
    let stored = required_context(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
    )
    .await?;
    let draft = stored.draft.as_ref().ok_or(Error::InvalidArguments)?;
    validate_review(request, draft)?;
    let next_revision = locked
        .revision
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    let review = ScopeCandidateReview {
        revision: next_revision,
        verdict: request.review.verdict,
        summary: request.review.summary.clone(),
        findings: request.review.findings.clone(),
        candidate_decisions: request.review.candidate_decisions.clone(),
        protected_change_reviews: request.review.protected_change_reviews.clone(),
    };
    let review_payload = serde_json::to_value(&review).map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO scope_candidate_reviews \
             (tenant_id,workspace_id,candidate_set_id,set_revision,payload) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(request.candidate_set_id)
    .bind(next_revision)
    .bind(&review_payload)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let status = match request.review.verdict {
        ReviewVerdict::Ready => CandidateSetStatus::Ready,
        ReviewVerdict::Revise => CandidateSetStatus::ReviewRequired,
        ReviewVerdict::Blocked => CandidateSetStatus::Blocked,
    };
    update_set(
        transaction,
        tenant_id,
        workspace_id,
        request.candidate_set_id,
        next_revision,
        status,
        request.input_cursor,
        locked.latest_input,
        None,
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
        "save_review",
        request.request_id,
        request_payload,
        next_revision,
        serde_json::to_value(&stored).map_err(storage_error)?,
    )
    .await?;
    Ok(stored)
}

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
