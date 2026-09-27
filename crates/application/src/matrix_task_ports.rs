use async_trait::async_trait;
use tect_domain::{Error, Result};
use uuid::Uuid;

use crate::{
    MatrixTaskRequirementsBinding, MatrixTaskRevision, MatrixTaskSource, RecordMatrixTask,
};

#[async_trait]
pub trait MatrixTaskStore: Send {
    /// Read the immutable receipt by request ID before resolving a possibly
    /// changed requirements context. Adapters without bound storage fail closed.
    async fn matrix_task_source_by_request(
        &mut self,
        _workspace_id: Uuid,
        _request_id: Uuid,
    ) -> Result<Option<(MatrixTaskSource, String)>> {
        Err(Error::Forbidden)
    }

    /// Atomically append the bound input, original-request digest, frozen
    /// snapshot reference, and task revision in this same unit of work. The
    /// implementation must enforce the existing task-head CAS and request-ID
    /// uniqueness, comparing the original digest on conflict/retry.
    async fn record_matrix_task_bound(
        &mut self,
        _workspace_id: Uuid,
        _principal_id: Uuid,
        _session_id: Uuid,
        _bound_request: &RecordMatrixTask,
        _canonical_bound_input: &serde_json::Value,
        _bound_input_digest: &str,
        _original_request_digest: &str,
        _binding: &MatrixTaskRequirementsBinding,
    ) -> Result<MatrixTaskSource> {
        Err(Error::Forbidden)
    }

    /// Current head plus its immutable optional context link. Historical
    /// unbound revisions remain readable with `requirements_binding: None`.
    async fn matrix_task_source(
        &mut self,
        _workspace_id: Uuid,
        _task_id: Uuid,
    ) -> Result<Option<MatrixTaskSource>> {
        Err(Error::Forbidden)
    }
    async fn record_matrix_task(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &RecordMatrixTask,
        canonical_input: &serde_json::Value,
        input_digest: &str,
    ) -> Result<MatrixTaskRevision>;

    async fn matrix_task(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<MatrixTaskRevision>>;

    /// Lock the task head and read its current immutable revision in this UoW.
    async fn lock_matrix_task(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<MatrixTaskRevision>>;
}
