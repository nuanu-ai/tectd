use super::{Enrollment, generate_credential, hex_lower};
use crate::storage_error;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Transaction};
use tect_domain::{Error, HostAuth, Result};
use uuid::Uuid;

/// An uncommitted enrollment. Dropping it rolls back all database records.
pub struct PendingVerifierEnrollment {
    transaction: Transaction<'static, Postgres>,
    enrollment: Enrollment,
}

impl PendingVerifierEnrollment {
    pub fn auth(&self) -> &HostAuth {
        &self.enrollment.auth
    }

    pub async fn try_commit(self) -> std::result::Result<Enrollment, VerifierCommitFailure> {
        match self.transaction.commit().await {
            Ok(()) => Ok(self.enrollment),
            Err(error) => Err(VerifierCommitFailure {
                enrollment: self.enrollment,
                database_rejected: matches!(error, sqlx::Error::Database(_)),
            }),
        }
    }
}

/// A server-reported COMMIT error proves rollback; a transport error does not.
#[derive(Debug)]
pub struct VerifierCommitFailure {
    pub enrollment: Enrollment,
    pub database_rejected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifierEnrollmentState {
    Committed,
    Absent,
    Inconsistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifierCommitDecision {
    RecoverSuccess,
    RemoveCredential,
    PreserveCredential,
}

pub fn resolve_verifier_commit(
    database_rejected: bool,
    state: Option<VerifierEnrollmentState>,
) -> VerifierCommitDecision {
    match state {
        Some(VerifierEnrollmentState::Committed) => VerifierCommitDecision::RecoverSuccess,
        Some(VerifierEnrollmentState::Absent) if database_rejected => {
            VerifierCommitDecision::RemoveCredential
        }
        _ => VerifierCommitDecision::PreserveCredential,
    }
}

/// Read through a fresh admin connection after a failed COMMIT acknowledgement.
pub async fn verifier_enrollment_state(
    pool: &PgPool,
    enrollment: &Enrollment,
    workspace_id: Uuid,
) -> Result<VerifierEnrollmentState> {
    let credential_digest = hex_lower(&Sha256::digest(enrollment.auth.credential.as_bytes()));
    let (principal, host, membership, exact): (bool, bool, bool, bool) = sqlx::query_as(
        "SELECT \
             EXISTS(SELECT 1 FROM principals WHERE id=$1), \
             EXISTS(SELECT 1 FROM hosts WHERE id=$2), \
             EXISTS(SELECT 1 FROM memberships WHERE principal_id=$1 AND workspace_id=$4), \
             EXISTS(SELECT 1 FROM principals p \
                    JOIN hosts h ON h.tenant_id=p.tenant_id AND h.principal_id=p.id \
                    JOIN memberships m ON m.tenant_id=p.tenant_id AND m.principal_id=p.id \
                    WHERE p.id=$1 AND p.tenant_id=$3 AND p.role='verifier' \
                      AND h.id=$2 AND h.credential_digest=$5 AND NOT h.revoked \
                      AND m.workspace_id=$4)",
    )
    .bind(enrollment.principal_id)
    .bind(enrollment.auth.host_id)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(credential_digest)
    .fetch_one(pool)
    .await
    .map_err(storage_error)?;
    Ok(if exact {
        VerifierEnrollmentState::Committed
    } else if !principal && !host && !membership {
        VerifierEnrollmentState::Absent
    } else {
        VerifierEnrollmentState::Inconsistent
    })
}

/// Operator-only enrollment into an existing tenant workspace. No owner membership
/// or verifier session is created. A commit failure retains the generated credential.
pub async fn prepare_verifier_enrollment(
    pool: &PgPool,
    tenant_id: Uuid,
    workspace_id: Uuid,
) -> Result<PendingVerifierEnrollment> {
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
    Ok(PendingVerifierEnrollment {
        transaction,
        enrollment: Enrollment {
            auth: HostAuth {
                host_id,
                credential,
            },
            tenant_id,
            principal_id,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncertain_commit_never_deletes_a_possible_credential() {
        assert_eq!(
            resolve_verifier_commit(false, Some(VerifierEnrollmentState::Absent)),
            VerifierCommitDecision::PreserveCredential
        );
        assert_eq!(
            resolve_verifier_commit(false, None),
            VerifierCommitDecision::PreserveCredential
        );
        assert_eq!(
            resolve_verifier_commit(true, Some(VerifierEnrollmentState::Inconsistent)),
            VerifierCommitDecision::PreserveCredential
        );
        assert_eq!(
            resolve_verifier_commit(true, Some(VerifierEnrollmentState::Absent)),
            VerifierCommitDecision::RemoveCredential
        );
        assert_eq!(
            resolve_verifier_commit(false, Some(VerifierEnrollmentState::Committed)),
            VerifierCommitDecision::RecoverSuccess
        );
    }
}
