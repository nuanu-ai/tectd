use super::{resolve, snapshot, write::*};
use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{
    AntiBloatApplyReceipt, CandidateReceiptRequest, CandidateSetStatus, CandidateSnapshotMaterial,
    Error, RecordCandidateInput, RefreshCandidateSet, Result, ReviewCandidateSet, ReviewVerdict,
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
    if request.selected_advisory.is_some() {
        return Err(Error::InvalidArguments);
    }
    save_draft_inner(
        transaction,
        tenant_id,
        workspace_id,
        request,
        DraftSaveMode::Ordinary,
    )
    .await
}

pub(crate) async fn save_selected_draft(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &SaveCandidateDraft,
    selected_material: &tect_domain::ResolvedCandidateDraft,
) -> Result<StoredCandidateContext> {
    if request.selected_advisory.is_none() {
        return Err(Error::InvalidArguments);
    }
    save_draft_inner(
        transaction,
        tenant_id,
        workspace_id,
        request,
        DraftSaveMode::Selected(selected_material),
    )
    .await
}

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

enum DraftSaveMode<'a> {
    Ordinary,
    Selected(&'a tect_domain::ResolvedCandidateDraft),
}

async fn save_draft_inner(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &SaveCandidateDraft,
    mode: DraftSaveMode<'_>,
) -> Result<StoredCandidateContext> {
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
        (&mode, locked.status),
        (
            DraftSaveMode::Ordinary,
            CandidateSetStatus::Ready | CandidateSetStatus::Blocked
        ) | (DraftSaveMode::Selected(_), CandidateSetStatus::Blocked)
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
    let resolved = match mode {
        DraftSaveMode::Selected(material) => material.clone(),
        DraftSaveMode::Ordinary => {
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
        }
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

include!("save/input_refresh.rs");
