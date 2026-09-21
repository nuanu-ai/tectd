use super::{validate_setup_roots, validate_source_roots};
use crate::storage_error;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use tect_domain::{Error, HostAuth, Result};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TenantIdentity {
    pub tenant_id: Uuid,
    pub principal_id: Uuid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostRegistration {
    pub host_id: Uuid,
    pub tenant_id: Uuid,
    pub principal_id: Uuid,
}

pub async fn ensure_tenant(pool: &PgPool, tenant_id: Uuid) -> Result<TenantIdentity> {
    if tenant_id.is_nil() {
        return Err(Error::InvalidArguments);
    }
    let mut transaction = pool.begin().await.map_err(storage_error)?;
    advisory_locks(&mut transaction, &[format!("tenant:{tenant_id}")]).await?;

    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1)")
        .bind(tenant_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(storage_error)?;
    let principal_id = if exists {
        existing_owner(&mut transaction, tenant_id).await?
    } else {
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
        principal_id
    };
    transaction.commit().await.map_err(storage_error)?;
    Ok(TenantIdentity {
        tenant_id,
        principal_id,
    })
}

pub async fn register_host(
    pool: &PgPool,
    tenant_id: Uuid,
    auth: &HostAuth,
    mut allowed_source_roots: Vec<String>,
    mut allowed_setup_roots: Vec<String>,
) -> Result<HostRegistration> {
    validate_registration(tenant_id, auth, &allowed_source_roots, &allowed_setup_roots)?;
    allowed_source_roots.sort_unstable();
    allowed_source_roots.dedup();
    allowed_setup_roots.sort_unstable();
    allowed_setup_roots.dedup();

    let credential_digest = hex_lower(&Sha256::digest(auth.credential.as_bytes()));
    let mut transaction = pool.begin().await.map_err(storage_error)?;
    advisory_locks(
        &mut transaction,
        &[
            format!("tenant:{tenant_id}"),
            format!("host:{}", auth.host_id),
            format!("credential:{credential_digest}"),
        ],
    )
    .await?;

    let tenant_exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tenants WHERE id=$1)")
            .bind(tenant_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(storage_error)?;
    if !tenant_exists {
        return Err(Error::NotFound);
    }
    let principal_id = existing_owner(&mut transaction, tenant_id).await?;

    type HostRow = (
        Uuid,
        Uuid,
        String,
        serde_json::Value,
        serde_json::Value,
        bool,
    );
    let existing: Option<HostRow> = sqlx::query_as(
        "SELECT tenant_id, principal_id, credential_digest, allowed_source_roots, allowed_setup_roots, revoked FROM hosts WHERE id=$1 FOR UPDATE",
    )
    .bind(auth.host_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(storage_error)?;
    if let Some((stored_tenant, stored_principal, stored_digest, source, setup, revoked)) = existing
    {
        let mut stored_source: Vec<String> =
            serde_json::from_value(source).map_err(storage_error)?;
        let mut stored_setup: Vec<String> = serde_json::from_value(setup).map_err(storage_error)?;
        stored_source.sort_unstable();
        stored_source.dedup();
        stored_setup.sort_unstable();
        stored_setup.dedup();
        if revoked
            || stored_tenant != tenant_id
            || stored_principal != principal_id
            || stored_digest != credential_digest
            || stored_source != allowed_source_roots
            || stored_setup != allowed_setup_roots
        {
            return Err(Error::Unauthorized);
        }
        transaction.commit().await.map_err(storage_error)?;
        return Ok(HostRegistration {
            host_id: auth.host_id,
            tenant_id,
            principal_id,
        });
    }

    let digest_host: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM hosts WHERE credential_digest=$1 FOR UPDATE")
            .bind(&credential_digest)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(storage_error)?;
    if digest_host.is_some() {
        return Err(Error::Unauthorized);
    }
    sqlx::query(
        "INSERT INTO hosts (id, tenant_id, principal_id, credential_digest, allowed_source_roots, allowed_setup_roots) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(auth.host_id)
    .bind(tenant_id)
    .bind(principal_id)
    .bind(credential_digest)
    .bind(sqlx::types::Json(&allowed_source_roots))
    .bind(sqlx::types::Json(&allowed_setup_roots))
    .execute(&mut *transaction)
    .await
    .map_err(storage_error)?;
    transaction.commit().await.map_err(storage_error)?;
    Ok(HostRegistration {
        host_id: auth.host_id,
        tenant_id,
        principal_id,
    })
}

fn validate_registration(
    tenant_id: Uuid,
    auth: &HostAuth,
    source_roots: &[String],
    setup_roots: &[String],
) -> Result<()> {
    if tenant_id.is_nil()
        || auth.host_id.is_nil()
        || auth.credential.len() != 64
        || !auth.credential.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Error::InvalidArguments);
    }
    validate_source_roots(source_roots)?;
    validate_setup_roots(setup_roots)
}

async fn existing_owner(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
) -> Result<Uuid> {
    let owners: Vec<Uuid> =
        sqlx::query_scalar("SELECT id FROM principals WHERE tenant_id=$1 AND role='owner'")
            .bind(tenant_id)
            .fetch_all(&mut **transaction)
            .await
            .map_err(storage_error)?;
    match owners.as_slice() {
        [principal_id] if !principal_id.is_nil() => Ok(*principal_id),
        _ => Err(Error::InternalInvariant),
    }
}

async fn advisory_locks(
    transaction: &mut Transaction<'_, Postgres>,
    keys: &[String],
) -> Result<()> {
    let mut keys = keys.to_vec();
    keys.sort_unstable();
    keys.dedup();
    for key in keys {
        sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended($1, 0))")
            .bind(key)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_rejects_nil_ids_and_invalid_credentials_before_storage() {
        let valid = HostAuth {
            host_id: Uuid::new_v4(),
            credential: "a".repeat(64),
        };
        assert_eq!(
            validate_registration(Uuid::nil(), &valid, &[], &[]),
            Err(Error::InvalidArguments)
        );
        for auth in [
            HostAuth {
                host_id: Uuid::nil(),
                credential: "a".repeat(64),
            },
            HostAuth {
                host_id: Uuid::new_v4(),
                credential: "a".repeat(63),
            },
            HostAuth {
                host_id: Uuid::new_v4(),
                credential: "g".repeat(64),
            },
        ] {
            assert_eq!(
                validate_registration(Uuid::new_v4(), &auth, &[], &[]),
                Err(Error::InvalidArguments)
            );
        }
    }
}
