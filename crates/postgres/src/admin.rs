use crate::storage_error;
use getrandom::fill;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tect_domain::{Error, HostAuth, Result};
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Debug)]
pub struct Enrollment {
    pub auth: HostAuth,
    pub tenant_id: Uuid,
    pub principal_id: Uuid,
}

/// Establish an operator-only pool without exposing SQLx through the CLI crate.
pub async fn connect_admin(url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(url)
        .await
        .map_err(storage_error)
}

pub async fn migrate(pool: &PgPool, runtime_role: &str) -> Result<()> {
    let quoted_role = quote_identifier(runtime_role)?;
    MIGRATOR.run(pool).await.map_err(storage_error)?;

    let role: Option<(bool, bool, bool)> = sqlx::query_as(
        r#"
        SELECT r.rolsuper,
               r.rolbypassrls,
               EXISTS (
                   SELECT 1 FROM pg_catalog.pg_database d
                   WHERE d.datname=pg_catalog.current_database() AND pg_catalog.pg_has_role(r.oid, d.datdba, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1 FROM pg_catalog.pg_namespace n
                   WHERE n.nspname='public' AND pg_catalog.pg_has_role(r.oid, n.nspowner, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1
                   FROM pg_catalog.pg_class c
                   JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
                   WHERE n.nspname='public'
                     AND c.relname IN (
                         'tenants', 'principals', 'hosts', 'workspaces', 'memberships',
                         'agent_sessions', 'source_repositories', 'source_worktrees',
                         'session_worktrees', 'workspace_events'
                     )
                     AND pg_catalog.pg_has_role(r.oid, c.relowner, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1
                   FROM pg_catalog.pg_proc p
                   JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
                   WHERE n.nspname='public'
                     AND p.proname='tect_authenticate_host'
                     AND pg_catalog.pg_has_role(r.oid, p.proowner, 'MEMBER')
               )
        FROM pg_catalog.pg_roles r
        WHERE r.rolname=$1
        "#,
    )
    .bind(runtime_role)
    .fetch_optional(pool)
    .await
    .map_err(storage_error)?;
    let (superuser, bypass_rls, owns_database_object) = role.ok_or(Error::InvalidConfiguration)?;
    if superuser || bypass_rls || owns_database_object {
        return Err(Error::InvalidConfiguration);
    }

    let mut transaction = pool.begin().await.map_err(storage_error)?;
    let statements = [
        format!("GRANT USAGE ON SCHEMA public TO {quoted_role}"),
        format!("REVOKE ALL PRIVILEGES ON TABLE tenants, principals, hosts FROM {quoted_role}"),
        format!(
            "GRANT SELECT, INSERT ON TABLE workspaces, memberships, \
             agent_sessions, workspace_events TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE source_repositories, source_worktrees \
             TO {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT, DELETE ON TABLE session_worktrees TO {quoted_role}"),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_authenticate_host(uuid, text, boolean) TO {quoted_role}"
        ),
    ];
    for statement in statements {
        sqlx::query(&statement)
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
    }
    transaction.commit().await.map_err(storage_error)
}

pub async fn enroll_host(
    pool: &PgPool,
    tenant_id: Option<Uuid>,
    allowed_source_roots: Vec<String>,
) -> Result<Enrollment> {
    validate_source_roots(&allowed_source_roots)?;
    let credential = generate_credential()?;
    let credential_digest = hex_lower(&Sha256::digest(credential.as_bytes()));
    let host_id = Uuid::new_v4();
    let mut transaction = pool.begin().await.map_err(storage_error)?;

    let (tenant_id, principal_id) = match tenant_id {
        Some(tenant_id) => {
            let principal_id: Option<Uuid> = sqlx::query_scalar(
                "SELECT id FROM principals WHERE tenant_id=$1 AND role='owner' FOR SHARE",
            )
            .bind(tenant_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(storage_error)?;
            (tenant_id, principal_id.ok_or(Error::NotFound)?)
        }
        None => {
            let tenant_id = Uuid::new_v4();
            let principal_id = Uuid::new_v4();
            sqlx::query("INSERT INTO tenants (id) VALUES ($1)")
                .bind(tenant_id)
                .execute(&mut *transaction)
                .await
                .map_err(storage_error)?;
            sqlx::query("INSERT INTO principals (id, tenant_id, role) VALUES ($1, $2, 'owner')")
                .bind(principal_id)
                .bind(tenant_id)
                .execute(&mut *transaction)
                .await
                .map_err(storage_error)?;
            (tenant_id, principal_id)
        }
    };

    sqlx::query(
        "INSERT INTO hosts \
             (id, tenant_id, principal_id, credential_digest, allowed_source_roots) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(host_id)
    .bind(tenant_id)
    .bind(principal_id)
    .bind(credential_digest)
    .bind(sqlx::types::Json(&allowed_source_roots))
    .execute(&mut *transaction)
    .await
    .map_err(storage_error)?;
    transaction.commit().await.map_err(storage_error)?;

    Ok(Enrollment {
        auth: HostAuth {
            host_id,
            credential,
        },
        tenant_id,
        principal_id,
    })
}

fn quote_identifier(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 63
        || !value.as_bytes()[0].is_ascii_alphabetic() && value.as_bytes()[0] != b'_'
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(Error::InvalidArguments);
    }
    Ok(format!("\"{value}\""))
}

fn validate_source_roots(roots: &[String]) -> Result<()> {
    if roots
        .iter()
        .any(|root| root.is_empty() || root.as_bytes().contains(&0))
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn generate_credential() -> Result<String> {
    let mut bytes = [0_u8; 32];
    fill(&mut bytes).map_err(storage_error)?;
    Ok(hex_lower(&bytes))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}
