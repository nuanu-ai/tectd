pub(crate) mod change;
pub(crate) mod context;
pub(crate) mod manifest;
pub(crate) mod publish;
mod rdf;

use crate::storage_error;
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use tect_domain::*;
use uuid::Uuid;

pub(crate) fn json<T: Serialize + ?Sized>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(storage_error)
}

pub(crate) fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(storage_error)
}

pub(crate) fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(storage_error)?;
    Ok(hex(&Sha256::digest(bytes)))
}

pub(crate) fn sha256(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
}

pub(crate) fn fingerprint(value: &KnowledgeConstraintDraft) -> Result<String> {
    let mut normalized = value.clone();
    normalized.conditions.sort();
    normalized.exceptions.sort();
    digest(&normalized)
}

fn hex(value: &[u8]) -> String {
    value.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn operation(value: KnowledgeOperation) -> &'static str {
    match value {
        KnowledgeOperation::Create => "create",
        KnowledgeOperation::Revise => "revise",
        KnowledgeOperation::Retract => "retract",
    }
}

pub(crate) fn stage(value: &str) -> Result<KnowledgeChangeStage> {
    match value {
        "review_required" => Ok(KnowledgeChangeStage::ReviewRequired),
        "ready_to_publish" => Ok(KnowledgeChangeStage::ReadyToPublish),
        "rejected" => Ok(KnowledgeChangeStage::Rejected),
        "committed" => Ok(KnowledgeChangeStage::Committed),
        _ => Err(Error::InternalInvariant),
    }
}

pub(crate) fn parse_operation(value: &str) -> Result<KnowledgeOperation> {
    match value {
        "create" => Ok(KnowledgeOperation::Create),
        "revise" => Ok(KnowledgeOperation::Revise),
        "retract" => Ok(KnowledgeOperation::Retract),
        _ => Err(Error::InternalInvariant),
    }
}

pub(crate) async fn ensure_state(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
) -> Result<(i64, bool, Option<String>)> {
    sqlx::query("SELECT tect_dk_ensure_workspace_state($1,$2)")
        .bind(tenant)
        .bind(workspace)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    sqlx::query_as("SELECT generation,capability_ready,pgrdf_version FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2")
        .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)
}

pub(crate) async fn lock_state(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
) -> Result<(i64, bool, Option<String>)> {
    let _ = ensure_state(tx, tenant, workspace).await?;
    sqlx::query_as("SELECT generation,capability_ready,pgrdf_version FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE")
        .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)
}

pub(crate) async fn publisher_gate(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended('tect-dk-native-publisher',0))")
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

pub(crate) async fn receipt<T: DeserializeOwned>(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    op: &str,
    request: Uuid,
    payload: &serde_json::Value,
) -> Result<Option<T>> {
    let row:Option<(serde_json::Value,serde_json::Value)>=sqlx::query_as("SELECT request_payload,result_payload FROM knowledge_command_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND operation=$3 AND request_id=$4")
        .bind(tenant).bind(workspace).bind(op).bind(request).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    match row {
        Some((stored, result)) if stored == *payload => Ok(Some(decode(result)?)),
        Some(_) => Err(Error::InputConflict),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_receipt<T: Serialize>(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    op: &str,
    request: Uuid,
    session: Uuid,
    payload: &serde_json::Value,
    result: &T,
) -> Result<()> {
    sqlx::query("INSERT INTO knowledge_command_receipts(tenant_id,workspace_id,operation,request_id,actor_session_id,request_payload,result_payload) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(tenant).bind(workspace).bind(op).bind(request).bind(session).bind(payload).bind(json(result)?).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

pub(crate) fn native_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("23514") | Some("22023") => Error::InvalidArguments,
        Some("42501") => Error::KnowledgeUnavailable,
        _ => Error::StorageUnavailable,
    }
}
