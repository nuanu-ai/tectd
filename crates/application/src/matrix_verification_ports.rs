use async_trait::async_trait;
use tect_domain::{MatrixEvidenceBinding, MatrixVerificationRecord, RequiredMatrixFact, Result};
use uuid::Uuid;

/// A host-owned validator must resolve immutable content, its SHA-256, source,
/// subject, observation time, expiry, and policy-specific trust and max age.
/// The caller supplies only a reference; no outcome or timestamps cross this API.
#[async_trait]
pub trait MatrixEvidenceValidator: Send + Sync {
    fn policy_version(&self) -> &str;

    async fn validate(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        fact: &RequiredMatrixFact,
        evidence_ref: &str,
        now: i64,
    ) -> Result<MatrixEvidenceBinding>;
}

pub struct DisabledMatrixEvidenceValidator;

#[async_trait]
impl MatrixEvidenceValidator for DisabledMatrixEvidenceValidator {
    fn policy_version(&self) -> &str {
        "unconfigured"
    }

    async fn validate(
        &self,
        _workspace_id: Uuid,
        _task_id: Uuid,
        _revision: i64,
        _fact: &RequiredMatrixFact,
        _evidence_ref: &str,
        _now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        Err(tect_domain::Error::Forbidden)
    }
}

/// Called within the same unit of work that locked and checked the task head.
/// Implementations must append atomically and reject a changed task head.
#[async_trait]
pub trait MatrixVerificationStore: Send {
    /// Returns an exact persisted record; callers must re-evaluate time-bound
    /// evidence at use time. None never implies verified.
    async fn matrix_verification_for_revision(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        input_digest: &str,
    ) -> Result<Option<MatrixVerificationRecord>>;

    async fn append_matrix_verification(
        &mut self,
        workspace_id: Uuid,
        verifier_session_id: Uuid,
        task_id: Uuid,
        expected_revision: i64,
        expected_input_digest: &str,
        record: &MatrixVerificationRecord,
    ) -> Result<()>;
}
