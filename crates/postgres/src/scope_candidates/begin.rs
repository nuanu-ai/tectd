use super::{boundary_name, load, snapshot};
use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{
    BeginCandidateSet, BeginCandidateSetOutcome, CandidateSnapshotMaterial, Error, Result,
};
use uuid::Uuid;

pub(crate) async fn replay(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    request: &BeginCandidateSet,
) -> Result<Option<BeginCandidateSetOutcome>> {
    let row: Option<(Uuid, Uuid, serde_json::Value, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT id,origin_request_id,origin_payload,origin_result FROM scope_candidate_sets \
         WHERE tenant_id=$1 AND workspace_id=$2 AND program_id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(request.program_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let Some((id, request_id, payload, origin_result)) = row else {
        return Ok(None);
    };
    if request_id == request.request_id {
        if payload != serde_json::to_value(request).map_err(storage_error)? {
            return Err(Error::InputConflict);
        }
        let context = origin_result.ok_or(Error::InternalInvariant)?;
        return serde_json::from_value(context)
            .map(BeginCandidateSetOutcome::Replay)
            .map(Some)
            .map_err(storage_error);
    }
    let context = load(transaction, tenant_id, workspace_id, id)
        .await?
        .ok_or(Error::InternalInvariant)?
        .context;
    Ok(Some(BeginCandidateSetOutcome::Existing(context)))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn ensure(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    session_id: Uuid,
    request: &BeginCandidateSet,
    input_bytes: i64,
    material: &CandidateSnapshotMaterial,
) -> Result<BeginCandidateSetOutcome> {
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(\
             $1::text||':'||$2::text||':'||$3::text,0))",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(request.program_id)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let payload = serde_json::to_value(request).map_err(storage_error)?;
    let existing: Option<(Uuid, Uuid, serde_json::Value, Option<serde_json::Value>)> =
        sqlx::query_as(
            "SELECT id,origin_request_id,origin_payload,origin_result FROM scope_candidate_sets \
         WHERE tenant_id=$1 AND workspace_id=$2 AND program_id=$3 FOR UPDATE",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(request.program_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(storage_error)?;
    if let Some((id, request_id, stored_payload, origin_result)) = existing {
        if request_id == request.request_id {
            if stored_payload == payload {
                let context = origin_result.ok_or(Error::InternalInvariant)?;
                return serde_json::from_value(context)
                    .map(BeginCandidateSetOutcome::Replay)
                    .map_err(storage_error);
            }
            return Err(Error::InputConflict);
        }
        let context = load(transaction, tenant_id, workspace_id, id)
            .await?
            .ok_or(Error::InternalInvariant)?
            .context;
        return Ok(BeginCandidateSetOutcome::Existing(context));
    }
    let candidate_set_id: Uuid = sqlx::query_scalar(
        "INSERT INTO scope_candidate_sets \
             (tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,\
              boundary,max_input_bytes) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(request.program_id)
    .bind(request.request_id)
    .bind(&request.input)
    .bind(payload)
    .bind(boundary_name(request.boundary))
    .bind(input_bytes)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO scope_candidate_inputs \
             (tenant_id,workspace_id,candidate_set_id,sequence,request_id,session_id,input) \
         VALUES ($1,$2,$3,1,$4,$5,$6)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(request.request_id)
    .bind(session_id)
    .bind(&request.input)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let snapshot_id = snapshot::insert(
        transaction,
        tenant_id,
        workspace_id,
        candidate_set_id,
        1,
        material,
    )
    .await?;
    sqlx::query(
        "UPDATE scope_candidate_sets SET current_snapshot_id=$4 \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(snapshot_id)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let context = load(transaction, tenant_id, workspace_id, candidate_set_id)
        .await?
        .ok_or(Error::InternalInvariant)?
        .context;
    sqlx::query(
        "UPDATE scope_candidate_sets SET origin_result=$4 \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(serde_json::to_value(&context).map_err(storage_error)?)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(BeginCandidateSetOutcome::Created(context))
}
