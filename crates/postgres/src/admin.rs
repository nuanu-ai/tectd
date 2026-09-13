use crate::storage_error;
use getrandom::fill;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tect_domain::{Error, HostAuth, Result, validate_setup_path};
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
                         'session_worktrees', 'workspace_events', 'programs', 'program_inputs',
                         'setup_session_directories', 'workspace_setups', 'workspace_setup_inputs',
                         'scope_candidate_sets', 'scope_candidate_inputs',
                         'scope_candidate_contents', 'scope_candidate_snapshots',
                         'scope_candidate_source_refs', 'scope_candidate_drafts',
                         'scope_candidate_reviews', 'scope_candidate_receipts',
                         'native_scopes', 'slice_candidate_sets', 'slice_planning_inputs',
                         'slice_planning_snapshots', 'slice_candidate_drafts',
                         'slice_candidate_reviews', 'native_slices', 'slice_results',
                         'native_planning_receipts', 'slice_pipeline_runs',
                         'slice_pipeline_phase_attempts', 'slice_pipeline_phase_outputs',
                         'slice_pipeline_output_bindings', 'slice_pipeline_inputs',
                         'slice_pipeline_receipts'
                     )
                     AND pg_catalog.pg_has_role(r.oid, c.relowner, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1
                   FROM pg_catalog.pg_proc p
                   JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
                   WHERE n.nspname='public'
                     AND p.proname IN ('tect_authenticate_host', 'tect_preserve_created_at')
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
        format!("REVOKE ALL PRIVILEGES ON TABLE programs, program_inputs FROM {quoted_role}"),
        format!("GRANT SELECT, INSERT, UPDATE ON TABLE programs TO {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE program_inputs TO {quoted_role}"),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE setup_session_directories, workspace_setups, \
             workspace_setup_inputs FROM {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT ON TABLE setup_session_directories TO {quoted_role}"),
        format!("GRANT SELECT, INSERT, UPDATE ON TABLE workspace_setups TO {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE workspace_setup_inputs TO {quoted_role}"),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE scope_candidate_sets, scope_candidate_inputs, \
             scope_candidate_contents, scope_candidate_snapshots, scope_candidate_source_refs, \
             scope_candidate_drafts, scope_candidate_reviews, scope_candidate_receipts \
             FROM {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT, UPDATE ON TABLE scope_candidate_sets TO {quoted_role}"),
        format!(
            "GRANT SELECT, INSERT ON TABLE scope_candidate_inputs, scope_candidate_contents, \
             scope_candidate_snapshots, scope_candidate_source_refs, scope_candidate_drafts, \
             scope_candidate_reviews, scope_candidate_receipts TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE native_scopes, slice_candidate_sets, \
             slice_planning_inputs, slice_planning_snapshots, slice_candidate_drafts, \
             slice_candidate_reviews, native_slices, slice_results, native_planning_receipts, \
             slice_pipeline_runs, slice_pipeline_phase_attempts, slice_pipeline_phase_outputs, \
             slice_pipeline_output_bindings, slice_pipeline_inputs, slice_pipeline_receipts \
             FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE ON TABLE native_scopes, slice_candidate_sets, \
             native_slices, slice_results TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE slice_planning_inputs, slice_planning_snapshots, \
             slice_candidate_drafts, slice_candidate_reviews, native_planning_receipts \
             TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE ON TABLE slice_pipeline_runs, \
             slice_pipeline_output_bindings TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE slice_pipeline_phase_attempts, \
             slice_pipeline_phase_outputs, slice_pipeline_inputs, \
             slice_pipeline_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE (result_payload) ON TABLE slice_pipeline_phase_attempts, \
             slice_pipeline_inputs TO {quoted_role}"
        ),
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
    enroll_host_with_grants(pool, tenant_id, allowed_source_roots, Vec::new()).await
}

pub async fn enroll_host_with_grants(
    pool: &PgPool,
    tenant_id: Option<Uuid>,
    allowed_source_roots: Vec<String>,
    allowed_setup_roots: Vec<String>,
) -> Result<Enrollment> {
    validate_source_roots(&allowed_source_roots)?;
    validate_setup_roots(&allowed_setup_roots)?;
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
             (id, tenant_id, principal_id, credential_digest, allowed_source_roots, \
              allowed_setup_roots) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(host_id)
    .bind(tenant_id)
    .bind(principal_id)
    .bind(credential_digest)
    .bind(sqlx::types::Json(&allowed_source_roots))
    .bind(sqlx::types::Json(&allowed_setup_roots))
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

pub async fn grant_setup_root(pool: &PgPool, host_id: Uuid, setup_root: String) -> Result<()> {
    if host_id.is_nil() {
        return Err(Error::InvalidArguments);
    }
    validate_setup_roots(std::slice::from_ref(&setup_root))?;

    let mut transaction = pool.begin().await.map_err(storage_error)?;
    let row: Option<(bool, serde_json::Value)> =
        sqlx::query_as("SELECT revoked, allowed_setup_roots FROM hosts WHERE id=$1 FOR UPDATE")
            .bind(host_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(storage_error)?;
    let (revoked, stored) = row.ok_or(Error::NotFound)?;
    if revoked {
        return Err(Error::Unauthorized);
    }

    let mut roots: Vec<String> = serde_json::from_value(stored).map_err(storage_error)?;
    if !roots.iter().any(|root| root == &setup_root) {
        roots.push(setup_root);
        let result = sqlx::query("UPDATE hosts SET allowed_setup_roots=$2 WHERE id=$1")
            .bind(host_id)
            .bind(sqlx::types::Json(&roots))
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(Error::StorageUnavailable);
        }
    }
    transaction.commit().await.map_err(storage_error)
}

pub async fn revoke_host(pool: &PgPool, host_id: Uuid) -> Result<()> {
    let mut transaction = pool.begin().await.map_err(storage_error)?;
    let revoked: Option<bool> =
        sqlx::query_scalar("SELECT revoked FROM hosts WHERE id=$1 FOR UPDATE")
            .bind(host_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(storage_error)?;
    let revoked = revoked.ok_or(Error::NotFound)?;
    if !revoked {
        sqlx::query("UPDATE hosts SET revoked=true WHERE id=$1")
            .bind(host_id)
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
    }
    transaction.commit().await.map_err(storage_error)
}

pub async fn revoke_session(pool: &PgPool, session_id: Uuid) -> Result<()> {
    let mut transaction = pool.begin().await.map_err(storage_error)?;
    let target: Option<(Uuid, String)> =
        sqlx::query_as("SELECT host_id, native_session_id FROM agent_sessions WHERE id=$1")
            .bind(session_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(storage_error)?;
    let (host_id, native_session_id) = target.ok_or(Error::NotFound)?;

    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
             pg_catalog.hashtextextended($1::text || ':' || $2, 0))",
    )
    .bind(host_id)
    .bind(native_session_id)
    .execute(&mut *transaction)
    .await
    .map_err(storage_error)?;

    sqlx::query("UPDATE agent_sessions SET revoked=true WHERE id=$1 AND NOT revoked")
        .bind(session_id)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
    transaction.commit().await.map_err(storage_error)
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

fn validate_setup_roots(roots: &[String]) -> Result<()> {
    if roots
        .iter()
        .any(|root| root.len() > 4096 || validate_setup_path(root).is_err())
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
