use async_trait::async_trait;
use tect_domain::{ContextMatrixVerificationRecord, Result};
use uuid::Uuid;

/// A V2 record shares the immutable verification identity with historical V1,
/// but is read through this distinct port so a bound task cannot consume V1.
#[async_trait]
pub trait ContextMatrixVerificationStore: Send {
    async fn context_matrix_verification_for_revision(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        input_digest: &str,
        frozen_snapshot_id: Uuid,
    ) -> Result<Option<ContextMatrixVerificationRecord>>;

    /// The caller has locked the task head and validated operating evidence.
    /// The adapter must repeat head, frozen binding, digest, and actor checks
    /// within this same unit of work before its immutable append.
    async fn append_context_matrix_verification(
        &mut self,
        workspace_id: Uuid,
        verifier_session_id: Uuid,
        task_id: Uuid,
        expected_revision: i64,
        expected_input_digest: &str,
        record: &ContextMatrixVerificationRecord,
    ) -> Result<()>;
}
