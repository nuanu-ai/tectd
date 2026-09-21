use crate::storage_error;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use tect_domain::*;
use uuid::Uuid;

fn json<T: Serialize>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(storage_error)
}

fn decode<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(storage_error)
}

fn digest<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(storage_error)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn mode(value: &str) -> Result<PipelineDeliveryMode> {
    decode(serde_json::Value::String(value.into()))
}

fn run_status(value: &str) -> Result<PipelineRunStatus> {
    decode(serde_json::Value::String(value.into()))
}

fn phase_outcome(value: &str) -> Result<PipelinePhaseOutcome> {
    decode(serde_json::Value::String(value.into()))
}

fn transition(value: &str) -> Result<PipelineTransition> {
    decode(serde_json::Value::String(value.into()))
}

fn pipeline(value: &str) -> Result<PipelineKind> {
    decode(serde_json::Value::String(value.into()))
}

fn enum_text<T: Serialize>(value: &T) -> Result<String> {
    match json(value)? {
        serde_json::Value::String(value) => Ok(value),
        _ => Err(Error::InternalInvariant),
    }
}

async fn session_principal(tx: &mut Transaction<'_, Postgres>, session: Uuid) -> Result<Uuid> {
    sqlx::query_scalar("SELECT tect_dk_session_principal($1)")
        .bind(session)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?
        .ok_or(Error::Forbidden)
}

mod checkpoint;
mod checkpoint_resolution;
mod context;
pub(crate) mod evidence_artifact;
mod input;
mod inquiry_contract;
mod knowledge_publication;
mod migration;
mod phase;
mod phase_validation;
mod run;

pub(crate) use checkpoint::{
    authorize_many as authorize_checkpoints, load_for_scope as load_checkpoints_for_scope,
    validate_candidate_lineage, validate_candidate_source,
};
pub(crate) use checkpoint_resolution::resolve as resolve_checkpoint;
pub(crate) use context::{load_context, load_context_without_delivery_receipt, load_output};
pub(crate) use input::{escalate_delivery, record_input};
pub(crate) use migration::migrate_run;
pub(crate) use phase::complete_phase;
pub(crate) use run::{begin, begin_replay};
