use super::{boundary_name, load, status_name};
use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{CandidateSetStatus, Error, Result, StoredCandidateContext};
use uuid::Uuid;

mod review_validation;
pub(super) use review_validation::validate_review;

#[cfg(test)]
mod review_diagnostic_tests;

pub(super) struct LockedSet {
    pub(super) revision: i64,
    pub(super) snapshot_id: Uuid,
    pub(super) input_cursor: i64,
    pub(super) latest_input: i64,
    pub(super) status: CandidateSetStatus,
}

pub(super) async fn lock_set(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<LockedSet> {
    let row: (i64, Option<Uuid>, i64, i64, String) = sqlx::query_as(
        "SELECT revision,current_snapshot_id,input_cursor,latest_input,status \
         FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    Ok(LockedSet {
        revision: row.0,
        snapshot_id: row.1.ok_or(Error::InternalInvariant)?,
        input_cursor: row.2,
        latest_input: row.3,
        status: parse_status(&row.4)?,
    })
}

pub(super) fn validate_write(
    locked: &LockedSet,
    revision: i64,
    snapshot: Uuid,
    cursor: i64,
) -> Result<()> {
    if locked.revision != revision {
        return Err(Error::StaleRevision);
    }
    if locked.snapshot_id != snapshot {
        return Err(Error::StaleContext);
    }
    if cursor != locked.latest_input {
        return Err(Error::InputPending);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update_set(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
    revision: i64,
    status: CandidateSetStatus,
    input_cursor: i64,
    latest_input: i64,
    snapshot: Option<Uuid>,
) -> Result<()> {
    sqlx::query(
        "UPDATE scope_candidate_sets SET revision=$4,status=$5,input_cursor=$6,latest_input=$7,\
             current_snapshot_id=COALESCE($8,current_snapshot_id) WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    ).bind(tenant_id).bind(workspace_id).bind(id).bind(revision).bind(status_name(status)).bind(input_cursor).bind(latest_input).bind(snapshot)
    .execute(&mut **transaction).await.map_err(storage_error)?;
    Ok(())
}

pub(super) async fn receipt(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
    operation: &str,
    request_id: Uuid,
    payload: &serde_json::Value,
) -> Result<Option<serde_json::Value>> {
    let row: Option<(Option<serde_json::Value>, Option<serde_json::Value>, bool)> = sqlx::query_as(
        "SELECT request_payload,result_payload,payload_erased FROM scope_candidate_receipts \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND operation=$4 AND request_id=$5",
    ).bind(tenant_id).bind(workspace_id).bind(id).bind(operation).bind(request_id)
    .fetch_optional(&mut **transaction).await.map_err(storage_error)?;
    match row {
        Some((_, _, true)) => Err(Error::KnowledgePayloadErased),
        Some((Some(stored), Some(result), false)) if stored == *payload => {
            let program: Uuid = sqlx::query_scalar(
                "SELECT program_id FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(id)
            .fetch_one(&mut **transaction)
            .await
            .map_err(storage_error)?;
            crate::planning_knowledge::require_owned_payload_identity(
                transaction,
                tenant_id,
                workspace_id,
                &["programs"],
                Some(program),
            )
            .await?;
            crate::planning_knowledge::require_owned_payload_identity(
                transaction,
                tenant_id,
                workspace_id,
                &["scope_candidate_receipts"],
                Some(id),
            )
            .await?;
            Ok(Some(result))
        }
        Some((Some(_), Some(_), false)) => Err(Error::InputConflict),
        Some(_) => Err(Error::InternalInvariant),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn insert_receipt(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
    operation: &str,
    request_id: Uuid,
    payload: serde_json::Value,
    revision: i64,
    result: serde_json::Value,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO scope_candidate_receipts \
             (tenant_id,workspace_id,candidate_set_id,operation,request_id,request_payload,result_revision,result_payload) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
    ).bind(tenant_id).bind(workspace_id).bind(id).bind(operation).bind(request_id).bind(payload).bind(revision).bind(result)
    .execute(&mut **transaction).await.map_err(storage_error)?;
    Ok(())
}

pub(super) async fn required_context(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<StoredCandidateContext> {
    load(transaction, tenant_id, workspace_id, id)
        .await?
        .ok_or(Error::InternalInvariant)
}

fn parse_status(value: &str) -> Result<CandidateSetStatus> {
    match value {
        "draft" => Ok(CandidateSetStatus::Draft),
        "review_required" => Ok(CandidateSetStatus::ReviewRequired),
        "ready" => Ok(CandidateSetStatus::Ready),
        "blocked" => Ok(CandidateSetStatus::Blocked),
        _ => Err(Error::StorageUnavailable),
    }
}

// Call only after locking the set and validating the expected revision/snapshot.
pub(super) async fn validate_draft_boundary(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    boundary: tect_domain::CandidateBoundary,
    previous: &StoredCandidateContext,
) -> Result<()> {
    let set = &previous.context.candidate_set;
    if boundary == set.boundary {
        return Ok(());
    }
    // Context-only refreshes may advance the revision before substantive planning.
    if set.status != CandidateSetStatus::Draft
        || previous.draft.is_some()
        || !previous.reviews.is_empty()
    {
        return Err(Error::InvalidArguments);
    }
    let opened_scope: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM native_scopes \
         WHERE tenant_id=$1 AND workspace_id=$2 AND source_candidate_set_id=$3)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(set.id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if opened_scope {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

// Atomically bind the corrected head, draft and receipt; preserve the begin.
pub(super) async fn update_draft_set(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &tect_domain::SaveCandidateDraft,
    next_revision: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE scope_candidate_sets SET revision=$4,status='review_required',input_cursor=$5,\
             boundary=$6 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(request.candidate_set_id)
    .bind(next_revision)
    .bind(request.input_cursor)
    .bind(boundary_name(request.draft.boundary))
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(())
}
