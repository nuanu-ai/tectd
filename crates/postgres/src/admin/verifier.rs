use super::{Enrollment, generate_credential, hex_lower};
use crate::storage_error;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tect_domain::{Error, HostAuth, Result};
use uuid::Uuid;

/// Operator-only enrollment into a workspace that already belongs to the tenant.
/// This creates no workspace or owner membership. Verifier sessions remain disabled.
pub async fn enroll_verifier(
    pool: &PgPool,
    tenant_id: Uuid,
    workspace_id: Uuid,
) -> Result<Enrollment> {
    if tenant_id.is_nil() || workspace_id.is_nil() {
        return Err(Error::InvalidArguments);
    }
    let credential = generate_credential()?;
    let credential_digest = hex_lower(&Sha256::digest(credential.as_bytes()));
    let principal_id = Uuid::new_v4();
    let host_id = Uuid::new_v4();
    let mut transaction = pool.begin().await.map_err(storage_error)?;

    let existing: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE tenant_id=$1 AND id=$2 FOR SHARE")
            .bind(tenant_id)
            .bind(workspace_id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(storage_error)?;
    if existing.is_none() {
        return Err(Error::NotFound);
    }

    sqlx::query("INSERT INTO principals (id, tenant_id, role) VALUES ($1, $2, 'verifier')")
        .bind(principal_id)
        .bind(tenant_id)
        .execute(&mut *transaction)
        .await
        .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO hosts (id, tenant_id, principal_id, credential_digest) VALUES ($1, $2, $3, $4)",
    )
    .bind(host_id)
    .bind(tenant_id)
    .bind(principal_id)
    .bind(credential_digest)
    .execute(&mut *transaction)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO memberships (tenant_id, workspace_id, principal_id) VALUES ($1, $2, $3)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(principal_id)
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
