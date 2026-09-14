use crate::storage_error;
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use tect_domain::*;
use uuid::Uuid;

mod basis;
mod begin;
mod commit;
mod commit_guard;
mod context;
pub(crate) mod erase;
mod erased_no_change;
mod event;
mod input;
mod phase;
mod phase_data;
mod plan;
mod promotion;
pub(crate) mod rdf;
mod settle;

pub(crate) use begin::begin;
pub(crate) use commit::commit;
pub(crate) use context::{current_knowledge_output, eligible_unit, lifecycle, load_context, unit};
pub(crate) use erased_no_change::{
    qualify_begin as qualify_erased_no_change, validate_current as validate_erased_no_change,
};
pub(crate) use event::verify_publication_event;
pub(crate) use input::record_input;
pub(crate) use phase::complete_phase;
pub(crate) use plan::compile_plan;
pub(crate) use settle::settle;

pub(crate) fn json<T: Serialize + ?Sized>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(storage_error)
}

pub(crate) fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(storage_error)
}

pub(crate) fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(storage_error)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub(crate) fn sha256_bytes(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn sealed_command_digest(
    plan: &KnowledgeBranchPlan,
    changeset: &KnowledgeProposedChangeset,
    evidence: &KnowledgeEvidenceManifest,
    checks: &KnowledgeObligationReceipts,
    impact: &KnowledgeImpactPlan,
    review: &KnowledgeReviewReceipt,
    baseline: &KnowledgeBaselineManifest,
    generation: i64,
    sealing_revision: i64,
) -> Result<String> {
    digest(&(
        plan,
        changeset,
        evidence,
        checks,
        impact,
        review,
        baseline,
        generation,
        sealing_revision,
    ))
}

pub(crate) fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    match json(value)? {
        serde_json::Value::String(value) => Ok(value),
        _ => Err(Error::InternalInvariant),
    }
}

pub(crate) async fn require_owner(
    tx: &mut Transaction<'_, Postgres>,
    principal: Uuid,
) -> Result<()> {
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if owner { Ok(()) } else { Err(Error::Forbidden) }
}

pub(crate) async fn lock_workspace(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
) -> Result<i64> {
    let (generation, ready, _version) =
        crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    if !ready {
        return Err(Error::KnowledgeUnavailable);
    }
    Ok(generation)
}

pub(crate) async fn lock_run(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
) -> Result<(i64, String, Option<String>)> {
    sqlx::query_as(
        "SELECT revision,status,current_phase_id FROM knowledge_change_runs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 AND id=$4 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .bind(run)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)
}

pub(crate) async fn replay<T: DeserializeOwned>(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    operation: &str,
    request_id: Uuid,
    request: &serde_json::Value,
) -> Result<Option<T>> {
    let row: Option<(
        Uuid,
        Option<serde_json::Value>,
        Option<serde_json::Value>,
        bool,
    )> = sqlx::query_as(
        "SELECT actor_principal_id,request_payload,result_payload,payload_erased \
             FROM knowledge_lifecycle_command_receipts \
             WHERE tenant_id=$1 AND workspace_id=$2 AND operation=$3 AND request_id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(operation)
    .bind(request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    match row {
        Some((actor, _, _, _)) if actor != principal => Err(Error::Forbidden),
        Some((_, _, _, true)) => Err(Error::KnowledgePayloadErased),
        Some((_, Some(stored), Some(result), false)) if stored == *request => {
            Ok(Some(decode(result)?))
        }
        Some((_, Some(_), Some(_), false)) => Err(Error::InputConflict),
        Some(_) => Err(Error::InternalInvariant),
        None => Ok(None),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_receipt<T: Serialize>(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    operation: &str,
    request_id: Uuid,
    request: &serde_json::Value,
    result: &T,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO knowledge_lifecycle_command_receipts \
         (tenant_id,workspace_id,operation,request_id,actor_principal_id,actor_session_id,request_payload,result_payload) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(operation)
    .bind(request_id)
    .bind(principal)
    .bind(session)
    .bind(request)
    .bind(json(result)?)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(crate) fn phase(value: &str) -> Result<KnowledgeChangePhaseId> {
    decode(serde_json::Value::String(value.into()))
}

pub(crate) fn run_status(value: &str) -> Result<PipelineRunStatus> {
    decode(serde_json::Value::String(value.into()))
}

pub(crate) fn phase_outcome(value: &str) -> Result<PipelinePhaseOutcome> {
    decode(serde_json::Value::String(value.into()))
}

pub(crate) fn transition(value: &str) -> Result<PipelineTransition> {
    decode(serde_json::Value::String(value.into()))
}
