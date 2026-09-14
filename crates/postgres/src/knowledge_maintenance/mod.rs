use crate::storage_error;
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use tect_domain::*;
use uuid::Uuid;

mod begin;
mod jobs;
mod lifecycle;
mod query;
mod registry;
mod signal;
mod source;
mod status;

pub(crate) use begin::begin_change;
pub(crate) use jobs::{claim, fail, pending, prepare, sweep_due};
pub(crate) use lifecycle::{publication_applied, reset_restored_leases};
pub(crate) use query::{context, linked_tasks};
pub(crate) use registry::{
    reconcile_unit_consumers, register_consumer, register_manifest_consumers, registered_consumers,
    retire_planning_owner_consumers,
};
pub(crate) use signal::observe;
pub(crate) use status::current_unit_review_status;

fn json<T: Serialize + ?Sized>(value: &T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(storage_error)
}

fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(storage_error)
}

fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(storage_error)?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

async fn require_owner(tx: &mut Transaction<'_, Postgres>, principal: Uuid) -> Result<()> {
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    owner.then_some(()).ok_or(Error::Forbidden)
}

async fn require_identity(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let ready: bool = sqlx::query_scalar("SELECT tect_dk_database_identity_ready()")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    ready.then_some(()).ok_or(Error::KnowledgeUnavailable)
}

async fn publisher_lock(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    crate::durable_knowledge::publisher_gate(tx).await
}
