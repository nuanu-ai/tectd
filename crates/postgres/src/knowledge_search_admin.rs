use crate::{knowledge_search, storage_error};
use sqlx::{PgPool, Postgres, Transaction};
use tect_domain::{Error, KNOWLEDGE_EMBEDDING_DIMENSIONS, Result};

const VECTOR_VERSION: &str = "0.8.6";

pub async fn enable_knowledge_vector_search(pool: &PgPool, runtime_role: &str) -> Result<()> {
    let role = quote_identifier(runtime_role)?;
    let mut tx = pool.begin().await.map_err(storage_error)?;
    assert_database_owner(&mut tx).await?;
    knowledge_search_identity_ready(&mut tx).await?;
    crate::durable_knowledge::publisher_gate(&mut tx).await?;
    install_vector(&mut tx).await?;
    create_vector_table(&mut tx).await?;
    let literal = unit_vector_literal();
    let distance: f64 = sqlx::query_scalar("SELECT ($1::vector <=> $1::vector)::double precision")
        .bind(&literal)
        .fetch_one(&mut *tx)
        .await
        .map_err(storage_error)?;
    if distance != 0.0 {
        return Err(Error::InvalidConfiguration);
    }
    let identity =
        crate::knowledge_recovery::current_knowledge_database_identity_in_tx(&mut tx).await?;
    sqlx::query("UPDATE knowledge_search_capability SET vector_ready=true,pgvector_version='0.8.6',model_name='intfloat/multilingual-e5-small',model_revision='614241f622f53c4eeff9890bdc4f31cfecc418b3',dimensions=384,recipe='title_v1',qualified_system_identifier=$1,qualified_database_oid=$2::bigint::oid,activated_at=pg_catalog.clock_timestamp() WHERE singleton")
        .bind(&identity.system_identifier).bind(i64::from(identity.database_oid))
        .execute(&mut *tx).await.map_err(storage_error)?;
    grant_search_runtime_in_tx(&mut tx, &role).await?;
    backfill_all(&mut tx).await?;
    tx.commit().await.map_err(storage_error)
}

pub(crate) async fn grant_search_runtime(
    tx: &mut Transaction<'_, Postgres>,
    runtime_role: &str,
) -> Result<()> {
    let role = quote_identifier(runtime_role)?;
    grant_search_runtime_in_tx(tx, &role).await
}

async fn grant_search_runtime_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    quoted_role: &str,
) -> Result<()> {
    for statement in [
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE knowledge_search_capability,knowledge_search_resources,knowledge_search_embedding_jobs FROM {quoted_role}"
        ),
        format!("GRANT SELECT ON TABLE knowledge_search_capability TO {quoted_role}"),
        format!(
            "GRANT SELECT,INSERT,UPDATE,DELETE ON TABLE knowledge_search_resources,knowledge_search_embedding_jobs TO {quoted_role}"
        ),
        format!("GRANT EXECUTE ON FUNCTION public.tect_dk_search_vector_ready() TO {quoted_role}"),
    ] {
        sqlx::query(&statement)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
    }
    let vector_table: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_vectors') IS NOT NULL",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if vector_table {
        for statement in [
            format!("REVOKE ALL PRIVILEGES ON TABLE knowledge_search_vectors FROM {quoted_role}"),
            format!(
                "GRANT SELECT,INSERT,UPDATE,DELETE ON TABLE knowledge_search_vectors TO {quoted_role}"
            ),
        ] {
            sqlx::query(&statement)
                .execute(&mut **tx)
                .await
                .map_err(storage_error)?;
        }
    }
    Ok(())
}

pub(crate) async fn backfill_all(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let rows: Vec<(uuid::Uuid, uuid::Uuid, uuid::Uuid, String, i64, uuid::Uuid)> = sqlx::query_as(
        "SELECT h.tenant_id,h.workspace_id,h.unit_id,h.contract_version,s.generation,(SELECT p.id FROM principals p WHERE p.tenant_id=h.tenant_id AND p.role='owner' ORDER BY p.id LIMIT 1) FROM knowledge_unit_heads h JOIN workspace_knowledge_state s ON s.tenant_id=h.tenant_id AND s.workspace_id=h.workspace_id WHERE h.lifecycle='active' AND h.active AND NOT h.payload_erased ORDER BY h.tenant_id,h.workspace_id,h.unit_id",
    ).fetch_all(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("DELETE FROM knowledge_search_embedding_jobs j WHERE NOT EXISTS(SELECT 1 FROM knowledge_unit_heads h WHERE h.tenant_id=j.tenant_id AND h.workspace_id=j.workspace_id AND h.unit_id=j.unit_id AND h.lifecycle='active' AND h.active AND NOT h.payload_erased)")
        .execute(&mut **tx).await.map_err(storage_error)?;
    let vector_table: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_vectors') IS NOT NULL",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if vector_table {
        sqlx::query("DELETE FROM knowledge_search_vectors v WHERE NOT EXISTS(SELECT 1 FROM knowledge_unit_heads h WHERE h.tenant_id=v.tenant_id AND h.workspace_id=v.workspace_id AND h.unit_id=v.unit_id AND h.lifecycle='active' AND h.active AND NOT h.payload_erased)")
            .execute(&mut **tx).await.map_err(storage_error)?;
    }
    sqlx::query("DELETE FROM knowledge_search_resources r WHERE NOT EXISTS(SELECT 1 FROM knowledge_unit_heads h WHERE h.tenant_id=r.tenant_id AND h.workspace_id=r.workspace_id AND h.unit_id=r.unit_id AND h.lifecycle='active' AND h.active AND NOT h.payload_erased)")
        .execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_lifecycle::erase::reconcile_absent_search_copies(tx).await?;
    for (tenant, workspace, unit, contract, generation, principal) in rows {
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(tenant.to_string())
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
        match contract.as_str() {
            "dk-1" => {
                knowledge_search::project_legacy(tx, tenant, workspace, principal, unit, generation)
                    .await?
            }
            "dk-2" => {
                knowledge_search::apply_dk2_operation(
                    tx, tenant, workspace, principal, unit, generation,
                )
                .await?
            }
            _ => return Err(Error::InvalidConfiguration),
        }
    }
    Ok(())
}

pub(crate) async fn qualify_restored_search(
    tx: &mut Transaction<'_, Postgres>,
    identity: &crate::KnowledgeDatabaseIdentity,
) -> Result<()> {
    let ready: bool = sqlx::query_scalar(
        "SELECT vector_ready FROM knowledge_search_capability WHERE singleton FOR UPDATE",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query("UPDATE knowledge_search_embedding_jobs SET state='pending',lease_token=NULL,lease_expires_at=NULL,available_at=pg_catalog.clock_timestamp(),updated_at=pg_catalog.clock_timestamp() WHERE state='leased'")
        .execute(&mut **tx).await.map_err(storage_error)?;
    if !ready {
        return Ok(());
    }
    install_vector(tx).await?;
    validate_vector_table(tx).await?;
    sqlx::query("UPDATE knowledge_search_capability SET qualified_system_identifier=$1,qualified_database_oid=$2::bigint::oid,activated_at=pg_catalog.clock_timestamp() WHERE singleton")
        .bind(&identity.system_identifier).bind(i64::from(identity.database_oid))
        .execute(&mut **tx).await.map_err(storage_error)?;
    backfill_all(tx).await
}

async fn knowledge_search_identity_ready(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let ready: bool = sqlx::query_scalar("SELECT tect_dk_database_identity_ready()")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    ready.then_some(()).ok_or(Error::KnowledgeUnavailable)
}

async fn assert_database_owner(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let owner: bool = sqlx::query_scalar("SELECT pg_catalog.pg_has_role(CURRENT_USER,d.datdba,'MEMBER') FROM pg_catalog.pg_database d WHERE d.datname=pg_catalog.current_database()")
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    owner.then_some(()).ok_or(Error::Forbidden)
}

async fn install_vector(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let existing: Option<(String,String,String)> = sqlx::query_as("SELECT e.extversion,r.rolname,n.nspname FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_roles r ON r.oid=e.extowner JOIN pg_catalog.pg_namespace n ON n.oid=e.extnamespace WHERE e.extname='vector'")
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if existing.is_none() {
        sqlx::query("CREATE EXTENSION vector VERSION '0.8.6' SCHEMA public")
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
    }
    let current: String = sqlx::query_scalar("SELECT CURRENT_USER")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let (version, owner, schema): (String,String,String) = sqlx::query_as("SELECT e.extversion,r.rolname,n.nspname FROM pg_catalog.pg_extension e JOIN pg_catalog.pg_roles r ON r.oid=e.extowner JOIN pg_catalog.pg_namespace n ON n.oid=e.extnamespace WHERE e.extname='vector'")
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if version != VECTOR_VERSION || owner != current || schema != "public" {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

async fn create_vector_table(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    sqlx::query("CREATE TABLE IF NOT EXISTS knowledge_search_vectors(tenant_id uuid NOT NULL,workspace_id uuid NOT NULL,unit_id uuid NOT NULL,revision bigint NOT NULL CHECK(revision>=1),access_scope text NOT NULL CHECK(access_scope IN('workspace_members','owners_only')),model_name text NOT NULL,model_revision text NOT NULL,dimensions integer NOT NULL CHECK(dimensions=384),recipe text NOT NULL,input_digest text NOT NULL,embedding vector(384) NOT NULL,updated_at timestamptz NOT NULL DEFAULT pg_catalog.clock_timestamp(),PRIMARY KEY(tenant_id,workspace_id,unit_id),FOREIGN KEY(tenant_id,workspace_id,unit_id) REFERENCES knowledge_unit_heads(tenant_id,workspace_id,unit_id) ON DELETE CASCADE)")
        .execute(&mut **tx).await.map_err(storage_error)?;
    for statement in [
        "ALTER TABLE knowledge_search_vectors ENABLE ROW LEVEL SECURITY",
        "ALTER TABLE knowledge_search_vectors FORCE ROW LEVEL SECURITY",
        "REVOKE ALL PRIVILEGES ON TABLE knowledge_search_vectors FROM PUBLIC",
    ] {
        sqlx::query(statement)
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
    }
    let policy: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_policies WHERE schemaname='public' AND tablename='knowledge_search_vectors' AND policyname='knowledge_search_vectors_tenant')")
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !policy {
        sqlx::query("CREATE POLICY knowledge_search_vectors_tenant ON knowledge_search_vectors USING(CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_search_vectors'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid) WITH CHECK(CURRENT_USER=pg_catalog.pg_get_userbyid((SELECT relowner FROM pg_catalog.pg_class WHERE oid='knowledge_search_vectors'::regclass)) OR tenant_id=NULLIF(pg_catalog.current_setting('tect.tenant_id',true),'')::uuid)")
            .execute(&mut **tx).await.map_err(storage_error)?;
    }
    validate_vector_table(tx).await
}

async fn validate_vector_table(tx: &mut Transaction<'_, Postgres>) -> Result<()> {
    let owner: bool = sqlx::query_scalar("SELECT pg_catalog.pg_has_role(CURRENT_USER,c.relowner,'MEMBER') FROM pg_catalog.pg_class c WHERE c.oid='knowledge_search_vectors'::regclass")
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    owner.then_some(()).ok_or(Error::InvalidConfiguration)
}

fn unit_vector_literal() -> String {
    let mut values = vec!["0"; KNOWLEDGE_EMBEDDING_DIMENSIONS as usize];
    values[0] = "1";
    format!("[{}]", values.join(","))
}

fn quote_identifier(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 63
        || (!value.as_bytes()[0].is_ascii_alphabetic() && value.as_bytes()[0] != b'_')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(Error::InvalidArguments);
    }
    Ok(format!("\"{value}\""))
}
