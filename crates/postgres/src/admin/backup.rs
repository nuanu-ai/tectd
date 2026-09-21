use super::{MIGRATOR, quote_identifier};
use crate::storage_error;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::BTreeSet;
use tect_domain::{Error, Result};

const PUBLISHER_LOCK: &str = "SELECT pg_catalog.pg_advisory_xact_lock_shared(pg_catalog.hashtextextended('tect-dk-native-publisher',0))";
const PUBLISHER_EXCLUSIVE_LOCK: &str = "SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended('tect-dk-native-publisher',0))";

#[derive(Clone, Debug)]
pub struct BackupIdentity {
    pub source_database: String,
    pub postgres_version_num: i32,
    pub pgrdf_version: String,
    pub pgrdf_build_id: String,
    pub schema_version: i64,
}

#[derive(Clone, Debug)]
pub struct BackupGraph {
    pub iri: String,
    pub native_digest: String,
    pub payload: String,
}

#[derive(Clone, Debug)]
pub struct RestoreGraph {
    pub iri: String,
    pub native_digest: String,
    pub payload: String,
}

pub struct BackupSnapshot {
    guard: Transaction<'static, Postgres>,
    snapshot: Transaction<'static, Postgres>,
    snapshot_id: String,
    identity: BackupIdentity,
}

impl BackupSnapshot {
    pub fn snapshot_id(&self) -> &str {
        &self.snapshot_id
    }

    pub fn identity(&self) -> &BackupIdentity {
        &self.identity
    }

    pub async fn export_graphs(&mut self) -> Result<Vec<BackupGraph>> {
        let inventory: Vec<(i64, String)> =
            sqlx::query_as("SELECT graph_id,iri FROM pgrdf.graph_inventory() ORDER BY iri")
                .fetch_all(&mut *self.snapshot)
                .await
                .map_err(storage_error)?;
        if inventory
            .iter()
            .any(|(_, iri)| iri.starts_with("urn:tect:dk:scratch:"))
        {
            return Err(Error::InvalidConfiguration);
        }
        let mut graphs = Vec::with_capacity(inventory.len());
        for (graph_id, iri) in inventory {
            let lines: Vec<String> = sqlx::query_scalar("SELECT * FROM pgrdf.export_graph($1)")
                .bind(graph_id)
                .fetch_all(&mut *self.snapshot)
                .await
                .map_err(storage_error)?;
            let payload = if lines.is_empty() {
                String::new()
            } else {
                lines.join("\n") + "\n"
            };
            let native_digest: String = sqlx::query_scalar("SELECT pgrdf.graph_digest($1)")
                .bind(graph_id)
                .fetch_one(&mut *self.snapshot)
                .await
                .map_err(storage_error)?;
            graphs.push(BackupGraph {
                iri,
                native_digest,
                payload,
            });
        }
        Ok(graphs)
    }

    pub async fn finish(self) -> Result<()> {
        self.snapshot.commit().await.map_err(storage_error)?;
        self.guard.commit().await.map_err(storage_error)
    }
}

pub async fn begin_backup_snapshot(pool: &PgPool) -> Result<BackupSnapshot> {
    let mut guard = pool.begin().await.map_err(storage_error)?;
    sqlx::query("SET TRANSACTION READ ONLY")
        .execute(&mut *guard)
        .await
        .map_err(storage_error)?;
    sqlx::query(PUBLISHER_LOCK)
        .execute(&mut *guard)
        .await
        .map_err(storage_error)?;

    let mut snapshot = pool.begin().await.map_err(storage_error)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *snapshot)
        .await
        .map_err(storage_error)?;
    let snapshot_id: String = sqlx::query_scalar("SELECT pg_catalog.pg_export_snapshot()")
        .fetch_one(&mut *snapshot)
        .await
        .map_err(storage_error)?;
    let (source_database, version_text, pgrdf_version, pgrdf_build_id, schema_version): (
        String,
        String,
        String,
        String,
        i64,
    ) = sqlx::query_as(
        "SELECT pg_catalog.current_database(),pg_catalog.current_setting('server_version_num'),\
         pgrdf.version(),pgrdf.build_id(),COALESCE((SELECT max(version) FROM _sqlx_migrations),0)",
    )
    .fetch_one(&mut *snapshot)
    .await
    .map_err(storage_error)?;
    let postgres_version_num = version_text
        .parse::<i32>()
        .map_err(|_| Error::InvalidConfiguration)?;
    if postgres_version_num / 10_000 != 18
        || pgrdf_version != "0.6.34"
        || pgrdf_build_id != "v0.6.34"
        || schema_version != current_schema_version()
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(BackupSnapshot {
        guard,
        snapshot,
        snapshot_id,
        identity: BackupIdentity {
            source_database,
            postgres_version_num,
            pgrdf_version,
            pgrdf_build_id,
            schema_version,
        },
    })
}

pub fn current_schema_version() -> i64 {
    MIGRATOR
        .migrations
        .last()
        .map_or(0, |migration| migration.version)
}

pub async fn validate_restore_preflight(pool: &PgPool, runtime_role: &str) -> Result<()> {
    super::validate_runtime_role(pool, runtime_role).await?;
    let (version_text, pgrdf_version): (String, Option<String>) = sqlx::query_as(
        "SELECT pg_catalog.current_setting('server_version_num'),
         (SELECT default_version FROM pg_catalog.pg_available_extensions WHERE name = 'pgrdf')",
    )
    .fetch_one(pool)
    .await
    .map_err(storage_error)?;
    let postgres_version_num = version_text
        .parse::<i32>()
        .map_err(|_| Error::InvalidConfiguration)?;
    if postgres_version_num / 10_000 != 18 || pgrdf_version.as_deref() != Some("0.6.34") {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

pub async fn create_restore_database(
    pool: &PgPool,
    database: &str,
    runtime_role: &str,
) -> Result<()> {
    let quoted = quote_identifier(database)?;
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname=$1)")
            .bind(database)
            .fetch_one(pool)
            .await
            .map_err(storage_error)?;
    if exists {
        return Err(Error::InputConflict);
    }
    sqlx::query(&format!(
        "CREATE DATABASE {quoted} WITH ALLOW_CONNECTIONS false"
    ))
    .execute(pool)
    .await
    .map_err(storage_error)?;
    if sqlx::query(&format!("REVOKE CONNECT ON DATABASE {quoted} FROM PUBLIC"))
        .execute(pool)
        .await
        .is_err()
    {
        return Err(Error::StorageUnavailable);
    }
    let runtime_connect: bool =
        sqlx::query_scalar("SELECT has_database_privilege($1, $2, 'CONNECT')")
            .bind(runtime_role)
            .bind(database)
            .fetch_one(pool)
            .await
            .map_err(storage_error)?;
    if runtime_connect {
        return Err(Error::InvalidConfiguration);
    }
    sqlx::query(&format!(
        "ALTER DATABASE {quoted} WITH ALLOW_CONNECTIONS true"
    ))
    .execute(pool)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub async fn restore_graphs(pool: &PgPool, graphs: &[RestoreGraph]) -> Result<()> {
    let mut expected = BTreeSet::new();
    if graphs.iter().any(|graph| {
        graph.iri.is_empty()
            || graph.iri.starts_with("urn:tect:dk:scratch:")
            || !expected.insert(graph.iri.clone())
    }) {
        return Err(Error::InvalidArguments);
    }
    let mut tx = pool.begin().await.map_err(storage_error)?;
    sqlx::query(PUBLISHER_EXCLUSIVE_LOCK)
        .execute(&mut *tx)
        .await
        .map_err(storage_error)?;
    for graph in graphs {
        let existing: Option<i64> = sqlx::query_scalar("SELECT pgrdf.graph_id($1)")
            .bind(&graph.iri)
            .fetch_one(&mut *tx)
            .await
            .map_err(storage_error)?;
        let graph_id = match existing {
            Some(graph_id) => graph_id,
            None => sqlx::query_scalar("SELECT pgrdf.add_graph($1)")
                .bind(&graph.iri)
                .fetch_one(&mut *tx)
                .await
                .map_err(storage_error)?,
        };
        let current: String = sqlx::query_scalar("SELECT pgrdf.graph_digest($1)")
            .bind(graph_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(storage_error)?;
        if current != graph.native_digest {
            let rows: Vec<String> = sqlx::query_scalar("SELECT * FROM pgrdf.export_graph($1)")
                .bind(graph_id)
                .fetch_all(&mut *tx)
                .await
                .map_err(storage_error)?;
            if !rows.is_empty() {
                return Err(Error::InputConflict);
            }
            if !graph.payload.is_empty() {
                sqlx::query("SELECT pgrdf.parse_turtle($1,$2)")
                    .bind(&graph.payload)
                    .bind(graph_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(storage_error)?;
            }
        }
        let restored: String = sqlx::query_scalar("SELECT pgrdf.graph_digest($1)")
            .bind(graph_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(storage_error)?;
        if restored != graph.native_digest {
            return Err(Error::InputConflict);
        }
    }
    let actual: Vec<String> =
        sqlx::query_scalar("SELECT iri FROM pgrdf.graph_inventory() ORDER BY iri")
            .fetch_all(&mut *tx)
            .await
            .map_err(storage_error)?;
    if actual.into_iter().collect::<BTreeSet<_>>() != expected {
        return Err(Error::InputConflict);
    }
    tx.commit().await.map_err(storage_error)
}

pub async fn grant_database_connect(
    pool: &PgPool,
    database: &str,
    runtime_role: &str,
) -> Result<()> {
    let database = quote_identifier(database)?;
    let role = quote_identifier(runtime_role)?;
    let safe_role: Option<(bool, bool)> =
        sqlx::query_as("SELECT rolsuper,rolbypassrls FROM pg_roles WHERE rolname=$1")
            .bind(runtime_role)
            .fetch_optional(pool)
            .await
            .map_err(storage_error)?;
    if safe_role != Some((false, false)) {
        return Err(Error::InvalidConfiguration);
    }
    sqlx::query(&format!("GRANT CONNECT ON DATABASE {database} TO {role}"))
        .execute(pool)
        .await
        .map_err(storage_error)?;
    Ok(())
}

pub async fn validate_restored_runtime_access(pool: &PgPool, runtime_role: &str) -> Result<()> {
    super::validate_runtime_role(pool, runtime_role).await?;
    let (capability, version, publish, read, native, connect):
        (bool, Option<String>, bool, bool, bool, bool) = sqlx::query_as(
        "SELECT c.capability_ready, c.pgrdf_version,
         has_function_privilege($1, 'public.tect_dk_native_publish(uuid,uuid,uuid,text,text,text)', 'EXECUTE'),
         has_function_privilege($1, 'public.tect_dk_native_read(uuid,uuid,uuid,bigint,uuid)', 'EXECUTE'),
         has_schema_privilege($1, 'pgrdf', 'USAGE') OR EXISTS(
           SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
           WHERE n.nspname = 'pgrdf' AND has_function_privilege($1, p.oid, 'EXECUTE')) OR EXISTS(
           SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
           WHERE n.nspname = 'pgrdf' AND CASE WHEN c.relkind = 'S'
             THEN has_sequence_privilege($1, c.oid, 'USAGE')
             ELSE has_table_privilege($1, c.oid, 'SELECT') END),
         has_database_privilege($1, current_database(), 'CONNECT')
         FROM durable_knowledge_capability c WHERE c.singleton",
    )
    .bind(runtime_role)
    .fetch_one(pool)
    .await
    .map_err(storage_error)?;
    if !capability || version.as_deref() != Some("0.6.34") || !publish || !read || native || connect
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
