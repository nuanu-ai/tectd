//! Operator-only suppression manifest export and managed restored-database recovery.
use crate::knowledge_lifecycle::{erase, rdf};
use crate::storage_error;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::BTreeSet;
use tect_domain::{Error, Result};
use uuid::Uuid;

const FORMAT_VERSION: &str = "tect-dk-suppression-v1";
pub type KnowledgeAdminPool = PgPool;
type SuppressionEntryRow = (Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, Uuid, i64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDatabaseIdentity {
    pub system_identifier: String,
    pub database_oid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSuppressionEntry {
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub unit_id: Uuid,
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub request_id: Uuid,
    pub event_id: Uuid,
    pub erasure_sequence: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSuppressionManifest {
    pub format_version: String,
    pub database_lineage_id: Uuid,
    pub high_water_erasure_sequence: i64,
    pub entries: Vec<KnowledgeSuppressionEntry>,
    pub manifest_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSuppressionCheckpoint {
    pub database_lineage_id: Uuid,
    pub erasure_sequence: i64,
    pub manifest_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeRecoveryReport {
    pub entries_applied: i64,
    pub units_suppressed: i64,
    pub native_triples_deleted: i64,
    pub native_dictionary_terms_deleted: i64,
    pub relational_rows_redacted: i64,
    pub remaining: i64,
    pub qualified_identity: KnowledgeDatabaseIdentity,
}

#[derive(Serialize)]
struct DigestPayload<'a> {
    format_version: &'a str,
    database_lineage_id: Uuid,
    high_water_erasure_sequence: i64,
    entries: &'a [KnowledgeSuppressionEntry],
}

impl KnowledgeSuppressionManifest {
    pub fn validate(&self) -> Result<()> {
        if self.format_version != FORMAT_VERSION
            || self.database_lineage_id.is_nil()
            || self.high_water_erasure_sequence < 0
            || self.entries.len() as i64 != self.high_water_erasure_sequence
            || self.manifest_digest.len() != 64
            || !self
                .manifest_digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::InvalidArguments);
        }
        let mut units = BTreeSet::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.erasure_sequence != index as i64 + 1
                || [
                    entry.tenant_id,
                    entry.workspace_id,
                    entry.unit_id,
                    entry.change_id,
                    entry.run_id,
                    entry.request_id,
                    entry.event_id,
                ]
                .iter()
                .any(Uuid::is_nil)
                || !units.insert((entry.tenant_id, entry.workspace_id, entry.unit_id))
            {
                return Err(Error::InvalidArguments);
            }
        }
        if digest(self)? != self.manifest_digest {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

pub fn parse_knowledge_suppression_manifest(bytes: &[u8]) -> Result<KnowledgeSuppressionManifest> {
    if bytes.is_empty() || bytes.len() > 8 * 1024 * 1024 {
        return Err(Error::InvalidArguments);
    }
    let manifest: KnowledgeSuppressionManifest =
        serde_json::from_slice(bytes).map_err(|_| Error::InvalidArguments)?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn knowledge_suppression_manifest_bytes(
    manifest: &KnowledgeSuppressionManifest,
) -> Result<Vec<u8>> {
    manifest.validate()?;
    let mut bytes = serde_json::to_vec_pretty(manifest).map_err(|_| Error::InternalInvariant)?;
    bytes.push(b'\n');
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(Error::CapacityExceeded);
    }
    Ok(bytes)
}

pub async fn current_knowledge_database_identity(
    pool: &PgPool,
) -> Result<KnowledgeDatabaseIdentity> {
    let mut tx = pool.begin().await.map_err(storage_error)?;
    let identity = current_knowledge_database_identity_in_tx(&mut tx).await?;
    tx.commit().await.map_err(storage_error)?;
    Ok(identity)
}

pub(crate) async fn current_knowledge_database_identity_in_tx(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<KnowledgeDatabaseIdentity> {
    let (system_identifier, database_oid): (String, i64) = sqlx::query_as(
        "SELECT system_identifier::text,(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=pg_catalog.current_database()) FROM pg_catalog.pg_control_system()",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(KnowledgeDatabaseIdentity {
        system_identifier,
        database_oid: u32::try_from(database_oid).map_err(|_| Error::InternalInvariant)?,
    })
}

pub async fn prepare_knowledge_suppression_manifest(
    pool: &PgPool,
) -> Result<KnowledgeSuppressionManifest> {
    let mut tx = pool.begin().await.map_err(storage_error)?;
    assert_database_owner(&mut tx).await?;
    publisher_gate(&mut tx).await?;
    let (lineage, high_water): (Uuid, i64) = sqlx::query_as(
        "SELECT database_lineage_id,erasure_sequence FROM durable_knowledge_capability WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(storage_error)?;
    assert_ledger_shape(&mut tx, high_water).await?;
    let entries = read_entries(&mut tx, high_water).await?;
    let manifest = make_manifest(lineage, high_water, entries)?;
    knowledge_suppression_manifest_bytes(&manifest)?;
    tx.commit().await.map_err(storage_error)?;
    Ok(manifest)
}

pub async fn record_knowledge_suppression_export(
    pool: &PgPool,
    manifest: &KnowledgeSuppressionManifest,
) -> Result<KnowledgeSuppressionCheckpoint> {
    manifest.validate()?;
    knowledge_suppression_manifest_bytes(manifest)?;
    let mut tx = pool.begin().await.map_err(storage_error)?;
    assert_database_owner(&mut tx).await?;
    publisher_gate(&mut tx).await?;
    let (lineage, high_water): (Uuid, i64) = sqlx::query_as(
        "SELECT database_lineage_id,erasure_sequence FROM durable_knowledge_capability WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(storage_error)?;
    if lineage != manifest.database_lineage_id || high_water < manifest.high_water_erasure_sequence
    {
        return Err(Error::StaleContext);
    }
    assert_ledger_shape(&mut tx, high_water).await?;
    let prefix = make_manifest(
        lineage,
        manifest.high_water_erasure_sequence,
        read_entries(&mut tx, manifest.high_water_erasure_sequence).await?,
    )?;
    if &prefix != manifest {
        return Err(Error::StaleContext);
    }
    sqlx::query("INSERT INTO knowledge_suppression_exports(database_lineage_id,erasure_sequence,manifest_digest) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(lineage).bind(manifest.high_water_erasure_sequence).bind(&manifest.manifest_digest)
        .execute(&mut *tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE durable_knowledge_capability SET exported_erasure_sequence=GREATEST(exported_erasure_sequence,$1),exported_manifest_digest=CASE WHEN exported_erasure_sequence<=$1 THEN $2 ELSE exported_manifest_digest END WHERE singleton")
        .bind(manifest.high_water_erasure_sequence).bind(&manifest.manifest_digest)
        .execute(&mut *tx).await.map_err(storage_error)?;
    tx.commit().await.map_err(storage_error)?;
    Ok(KnowledgeSuppressionCheckpoint {
        database_lineage_id: lineage,
        erasure_sequence: manifest.high_water_erasure_sequence,
        manifest_digest: manifest.manifest_digest.clone(),
    })
}

pub async fn apply_knowledge_suppression_manifest(
    pool: &PgPool,
    manifest: &KnowledgeSuppressionManifest,
    expected: &KnowledgeSuppressionCheckpoint,
) -> Result<KnowledgeRecoveryReport> {
    manifest.validate()?;
    knowledge_suppression_manifest_bytes(manifest)?;
    if expected.database_lineage_id != manifest.database_lineage_id
        || expected.erasure_sequence != manifest.high_water_erasure_sequence
        || expected.manifest_digest != manifest.manifest_digest
    {
        return Err(Error::InvalidArguments);
    }
    let mut tx = pool.begin().await.map_err(storage_error)?;
    assert_offline_owner(&mut tx).await?;
    publisher_gate(&mut tx).await?;
    let (lineage, restored_high_water, stored_system, stored_oid): (
        Uuid,
        i64,
        Option<String>,
        Option<i64>,
    ) = sqlx::query_as(
        "SELECT database_lineage_id,erasure_sequence,qualified_system_identifier,qualified_database_oid::bigint FROM durable_knowledge_capability WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(storage_error)?;
    if lineage != expected.database_lineage_id || restored_high_water > expected.erasure_sequence {
        return Err(Error::StaleContext);
    }
    assert_ledger_shape(&mut tx, restored_high_water).await?;
    let actual_identity = current_knowledge_database_identity_in_tx(&mut tx).await?;
    if stored_system.as_deref() == Some(&actual_identity.system_identifier)
        && stored_oid == Some(i64::from(actual_identity.database_oid))
    {
        let reapplied: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_suppression_recoveries WHERE database_lineage_id=$1 AND erasure_sequence=$2 AND manifest_digest=$3 AND qualified_system_identifier=$4 AND qualified_database_oid=$5::bigint::oid)")
            .bind(expected.database_lineage_id).bind(expected.erasure_sequence).bind(&expected.manifest_digest)
            .bind(&actual_identity.system_identifier).bind(i64::from(actual_identity.database_oid))
            .fetch_one(&mut *tx).await.map_err(storage_error)?;
        if !reapplied {
            return Err(Error::InvalidConfiguration);
        }
    }
    verify_restored_prefix(&mut tx, manifest, restored_high_water).await?;
    let mut inserted = 0i64;
    for entry in &manifest.entries {
        inserted += sqlx::query("INSERT INTO knowledge_suppression_ledger(tenant_id,workspace_id,unit_id,change_id,run_id,request_id,event_id,erasure_sequence,lifecycle,owned_live_copies_status,restore_safe_status) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'erased','pending','pending') ON CONFLICT DO NOTHING")
            .bind(entry.tenant_id).bind(entry.workspace_id).bind(entry.unit_id).bind(entry.change_id)
            .bind(entry.run_id).bind(entry.request_id).bind(entry.event_id).bind(entry.erasure_sequence)
            .execute(&mut *tx).await.map_err(storage_error)?.rows_affected() as i64;
    }
    verify_restored_prefix(&mut tx, manifest, manifest.high_water_erasure_sequence).await?;
    let mut report = KnowledgeRecoveryReport {
        entries_applied: inserted,
        units_suppressed: 0,
        native_triples_deleted: 0,
        native_dictionary_terms_deleted: 0,
        relational_rows_redacted: 0,
        remaining: 0,
        qualified_identity: actual_identity,
    };
    let mut touched = BTreeSet::new();
    for entry in &manifest.entries {
        let workspace_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM workspaces WHERE tenant_id=$1 AND id=$2)",
        )
        .bind(entry.tenant_id)
        .bind(entry.workspace_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
        let mut changed = false;
        if workspace_exists {
            sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
                .bind(entry.tenant_id.to_string())
                .execute(&mut *tx)
                .await
                .map_err(storage_error)?;
            let value = erase::suppress_owned_unit(
                &mut tx,
                entry.tenant_id,
                entry.workspace_id,
                entry.unit_id,
            )
            .await?;
            report.native_triples_deleted += value.native_triples_deleted;
            report.native_dictionary_terms_deleted += value.native_dictionary_terms_deleted;
            report.relational_rows_redacted += value.relational_rows_redacted;
            report.remaining += value.remaining;
            changed = value.native_triples_deleted != 0
                || value.native_dictionary_terms_deleted != 0
                || value.relational_rows_redacted != 0;
            if value.remaining != 0 {
                return Err(Error::KnowledgeUnavailable);
            }
        }
        let residual =
            erase::residual_owned_unit(&mut tx, entry.tenant_id, entry.workspace_id, entry.unit_id)
                .await?;
        report.remaining += residual.native_owned_triples + residual.relational_readable_rows;
        if !residual.complete {
            return Err(Error::KnowledgeUnavailable);
        }
        report.units_suppressed += 1;
        sqlx::query("UPDATE knowledge_suppression_ledger SET lifecycle='erased',owned_live_copies_status='ready',restore_safe_status='ready' WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(entry.tenant_id).bind(entry.workspace_id).bind(entry.unit_id)
            .execute(&mut *tx).await.map_err(storage_error)?;
        if changed {
            touched.insert((entry.tenant_id, entry.workspace_id));
        }
    }
    if report.remaining != 0 {
        return Err(Error::KnowledgeUnavailable);
    }
    for (tenant, workspace) in touched {
        sqlx::query("UPDATE workspace_knowledge_state SET generation=generation+1 WHERE tenant_id=$1 AND workspace_id=$2")
            .bind(tenant).bind(workspace).execute(&mut *tx).await.map_err(storage_error)?;
    }
    for statement in [
        "REVOKE ALL PRIVILEGES ON SCHEMA pgrdf FROM PUBLIC",
        "REVOKE ALL PRIVILEGES ON ALL TABLES IN SCHEMA pgrdf FROM PUBLIC",
        "REVOKE ALL PRIVILEGES ON ALL SEQUENCES IN SCHEMA pgrdf FROM PUBLIC",
        "REVOKE ALL PRIVILEGES ON ALL FUNCTIONS IN SCHEMA pgrdf FROM PUBLIC",
        "REVOKE ALL PRIVILEGES ON ALL ROUTINES IN SCHEMA pgrdf FROM PUBLIC",
    ] {
        sqlx::query(statement)
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
    }
    rdf::qualify_native(&mut tx).await?;
    sqlx::query("UPDATE durable_knowledge_capability SET erasure_sequence=$1,capability_ready=true,pgrdf_version='0.6.34',qualified_system_identifier=$2,qualified_database_oid=$3::bigint::oid,qualified_at=pg_catalog.clock_timestamp() WHERE singleton")
        .bind(manifest.high_water_erasure_sequence).bind(&report.qualified_identity.system_identifier)
        .bind(i64::from(report.qualified_identity.database_oid)).execute(&mut *tx).await.map_err(storage_error)?;
    crate::knowledge_search_admin::qualify_restored_search(&mut tx, &report.qualified_identity)
        .await?;
    sqlx::query("INSERT INTO knowledge_suppression_exports(database_lineage_id,erasure_sequence,manifest_digest) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(expected.database_lineage_id).bind(expected.erasure_sequence).bind(&expected.manifest_digest)
        .execute(&mut *tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE durable_knowledge_capability SET exported_erasure_sequence=$1,exported_manifest_digest=$2 WHERE singleton")
        .bind(expected.erasure_sequence).bind(&expected.manifest_digest)
        .execute(&mut *tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO knowledge_suppression_recoveries(database_lineage_id,erasure_sequence,manifest_digest,qualified_system_identifier,qualified_database_oid) VALUES($1,$2,$3,$4,$5::bigint::oid) ON CONFLICT DO NOTHING")
        .bind(expected.database_lineage_id).bind(expected.erasure_sequence).bind(&expected.manifest_digest)
        .bind(&report.qualified_identity.system_identifier).bind(i64::from(report.qualified_identity.database_oid))
        .execute(&mut *tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE workspace_knowledge_state SET capability_ready=true,pgrdf_version='0.6.34',activated_at=pg_catalog.clock_timestamp()")
        .execute(&mut *tx).await.map_err(storage_error)?;
    tx.commit().await.map_err(storage_error)?;
    Ok(report)
}

async fn publisher_gate(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended('tect-dk-native-publisher',0))")
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

async fn assert_offline_owner(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    assert_database_owner(tx).await?;
    let others: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE datid=(SELECT oid FROM pg_catalog.pg_database WHERE datname=pg_catalog.current_database()) AND pid<>pg_catalog.pg_backend_pid()")
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if others != 0 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

async fn assert_database_owner(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let owner: bool = sqlx::query_scalar("SELECT pg_catalog.pg_has_role(CURRENT_USER,d.datdba,'MEMBER') FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database()")
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !owner {
        return Err(Error::Forbidden);
    }
    Ok(())
}

async fn read_entries(
    tx: &mut Transaction<'_, Postgres>,
    high_water: i64,
) -> Result<Vec<KnowledgeSuppressionEntry>> {
    let rows: Vec<SuppressionEntryRow> = sqlx::query_as("SELECT tenant_id,workspace_id,unit_id,change_id,run_id,request_id,event_id,erasure_sequence FROM knowledge_suppression_ledger WHERE erasure_sequence<=$1 ORDER BY erasure_sequence")
        .bind(high_water).fetch_all(&mut **tx).await.map_err(storage_error)?;
    Ok(rows
        .into_iter()
        .map(|row| KnowledgeSuppressionEntry {
            tenant_id: row.0,
            workspace_id: row.1,
            unit_id: row.2,
            change_id: row.3,
            run_id: row.4,
            request_id: row.5,
            event_id: row.6,
            erasure_sequence: row.7,
        })
        .collect())
}

async fn assert_ledger_shape(tx: &mut Transaction<'_, Postgres>, high_water: i64) -> Result<()> {
    let (count, maximum): (i64, i64) = sqlx::query_as(
        "SELECT count(*),COALESCE(max(erasure_sequence),0) FROM knowledge_suppression_ledger",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if count != high_water || maximum != high_water {
        return Err(Error::StaleContext);
    }
    Ok(())
}

async fn verify_restored_prefix(
    tx: &mut Transaction<'_, Postgres>,
    manifest: &KnowledgeSuppressionManifest,
    high_water: i64,
) -> Result<()> {
    let restored = read_entries(tx, high_water).await?;
    if restored
        != manifest.entries[..usize::try_from(high_water).map_err(|_| Error::InternalInvariant)?]
    {
        return Err(Error::StaleContext);
    }
    Ok(())
}

fn make_manifest(
    lineage: Uuid,
    high_water: i64,
    entries: Vec<KnowledgeSuppressionEntry>,
) -> Result<KnowledgeSuppressionManifest> {
    let mut manifest = KnowledgeSuppressionManifest {
        format_version: FORMAT_VERSION.into(),
        database_lineage_id: lineage,
        high_water_erasure_sequence: high_water,
        entries,
        manifest_digest: String::new(),
    };
    manifest.manifest_digest = digest(&manifest)?;
    manifest.validate()?;
    Ok(manifest)
}

fn digest(manifest: &KnowledgeSuppressionManifest) -> Result<String> {
    let bytes = serde_json::to_vec(&DigestPayload {
        format_version: &manifest.format_version,
        database_lineage_id: manifest.database_lineage_id,
        high_water_erasure_sequence: manifest.high_water_erasure_sequence,
        entries: &manifest.entries,
    })
    .map_err(|_| Error::InternalInvariant)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
mod tests;
