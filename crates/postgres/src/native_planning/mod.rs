use crate::storage_error;
use sqlx::{Postgres, Transaction};
use std::collections::{BTreeMap, BTreeSet};
use tect_domain::*;
use uuid::Uuid;

fn json<T: serde::Serialize>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(storage_error)
}
fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(storage_error)
}
fn boundary(value: &str) -> Result<CandidateBoundary> {
    decode(serde_json::Value::String(value.into()))
}
fn set_status(value: &str) -> Result<SliceCandidateSetStatus> {
    decode(serde_json::Value::String(value.into()))
}
fn slice_state(value: &str) -> Result<SliceState> {
    decode(serde_json::Value::String(value.into()))
}
fn pipeline(value: &str) -> Result<PipelineKind> {
    decode(serde_json::Value::String(value.into()))
}
fn result_outcome(value: &str) -> Result<SliceResultOutcome> {
    decode(serde_json::Value::String(value.into()))
}

pub(super) async fn receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    entity: Uuid,
    operation: &str,
    request_id: Uuid,
    payload: &serde_json::Value,
) -> Result<Option<SliceCandidateContext>> {
    let row:Option<(serde_json::Value,serde_json::Value)>=sqlx::query_as("SELECT request_payload,result_payload FROM native_planning_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND entity_id=$3 AND operation=$4 AND request_id=$5")
        .bind(tenant).bind(workspace).bind(entity).bind(operation).bind(request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    match row {
        None => Ok(None),
        Some((stored, result)) => {
            if &stored != payload {
                return Err(Error::InputConflict);
            }
            Ok(Some(decode(result)?))
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) async fn save_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    entity: Uuid,
    operation: &str,
    request_id: Uuid,
    payload: serde_json::Value,
    result: &SliceCandidateContext,
) -> Result<()> {
    sqlx::query("INSERT INTO native_planning_receipts(tenant_id,workspace_id,entity_id,operation,request_id,request_payload,result_payload) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(tenant).bind(workspace).bind(entity).bind(operation).bind(request_id).bind(payload).bind(json(result)?).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

pub(super) async fn lock_set(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
    set: Uuid,
) -> Result<(i64, String, Uuid, i64, i64)> {
    sqlx::query_as("SELECT revision,status,current_snapshot_id,input_cursor,latest_input FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3 AND id=$4 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(scope).bind(set).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)
}
pub(super) fn guard(
    locked: &(i64, String, Uuid, i64, i64),
    revision: i64,
    snapshot: Uuid,
    cursor: i64,
) -> Result<()> {
    if locked.0 != revision {
        return Err(Error::StaleRevision);
    }
    if locked.2 != snapshot || locked.3 != cursor || locked.4 != cursor {
        return Err(Error::StaleContext);
    }
    Ok(())
}

pub(crate) async fn planning_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &NativePlanningReceiptRequest,
) -> Result<Option<SliceCandidateContext>> {
    let payload = match request {
        NativePlanningReceiptRequest::SaveDraft(value) => json(value)?,
        NativePlanningReceiptRequest::Review(value) => json(value)?,
        NativePlanningReceiptRequest::RecordInput(value) => json(value)?,
        NativePlanningReceiptRequest::Refresh(value) => json(value)?,
    };
    receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id(),
        request.operation(),
        request.request_id(),
        &payload,
    )
    .await
}

mod context;
mod continuation;
mod draft;
mod scope;
mod slice;

pub(crate) use context::{load_context, load_scope, load_slice, summaries};
pub(crate) use continuation::{record_input, refresh, save_review};
pub(crate) use draft::save_draft;
use scope::insert_snapshot;
pub(crate) use scope::{open_scope, scope_open_basis, scope_open_replay};
pub(crate) use slice::{open_slice, record_result};
