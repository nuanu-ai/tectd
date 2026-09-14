use super::{load, status_name};
use crate::storage_error;
use sqlx::{Postgres, Transaction};
use std::collections::BTreeSet;
use tect_domain::{
    CandidateDecisionKind, CandidateFindingSeverity, CandidateSetStatus, Error, Result,
    ReviewCandidateSet, ReviewVerdict, StoredCandidateContext,
};
use uuid::Uuid;

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

pub(super) fn validate_review(
    request: &ReviewCandidateSet,
    draft: &tect_domain::ResolvedCandidateDraft,
) -> Result<()> {
    if request.review.summary.trim().is_empty() || request.review.summary.contains('\0') {
        return Err(Error::InvalidArguments);
    }
    let candidate_ids: BTreeSet<_> = draft.candidates.iter().map(|v| v.id).collect();
    let decisions: BTreeSet<_> = request
        .review
        .candidate_decisions
        .iter()
        .map(|v| v.candidate_id)
        .collect();
    if decisions != candidate_ids
        || request
            .review
            .candidate_decisions
            .iter()
            .any(|v| v.rationale.trim().is_empty())
    {
        return Err(Error::InvalidArguments);
    }
    let protected: BTreeSet<_> = draft
        .protected_changes
        .iter()
        .map(|value| (value.accepted_evidence_id, value.prior_candidate_id))
        .collect();
    let reviewed: BTreeSet<_> = request
        .review
        .protected_change_reviews
        .iter()
        .map(|value| (value.accepted_evidence_id, value.prior_candidate_id))
        .collect();
    if protected != reviewed
        || request
            .review
            .protected_change_reviews
            .iter()
            .any(|value| value.rationale.trim().is_empty())
    {
        return Err(Error::InvalidArguments);
    }
    let goal_ids: BTreeSet<_> = draft.goals.iter().map(|v| v.id).collect();
    for finding in &request.review.findings {
        if finding.summary.trim().is_empty()
            || finding.disposition.trim().is_empty()
            || finding
                .candidate_ids
                .iter()
                .any(|id| !candidate_ids.contains(id))
            || finding
                .coverage_goal_ids
                .iter()
                .any(|id| !goal_ids.contains(id))
        {
            return Err(Error::InvalidArguments);
        }
    }
    let material = request
        .review
        .findings
        .iter()
        .any(|v| v.severity == CandidateFindingSeverity::Material);
    let empty_ready = draft.candidates.is_empty()
        && draft.empty_disposition.as_ref().is_some_and(|value| {
            value.kind == tect_domain::EmptyCandidateDispositionKind::AllCovered
        });
    let empty_blocked = draft.candidates.is_empty()
        && draft.empty_disposition.as_ref().is_some_and(|value| {
            matches!(
                value.kind,
                tect_domain::EmptyCandidateDispositionKind::NeedsInput
                    | tect_domain::EmptyCandidateDispositionKind::OutOfBoundary
            )
        });
    match request.review.verdict {
        ReviewVerdict::Ready
            if !draft.blockers.is_empty()
                || material
                || draft.pending_question.is_some()
                || draft.candidates.is_empty() && !empty_ready
                || request
                    .review
                    .candidate_decisions
                    .iter()
                    .any(|v| v.decision != CandidateDecisionKind::Accept) =>
        {
            Err(Error::InvalidArguments)
        }
        ReviewVerdict::Blocked if draft.blockers.is_empty() && !material && !empty_blocked => {
            Err(Error::InvalidArguments)
        }
        _ => Ok(()),
    }
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
