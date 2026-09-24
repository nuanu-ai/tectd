use async_trait::async_trait;
use tect_domain::Result;
use uuid::Uuid;

use crate::{MatrixTaskRevision, RecordMatrixTask};

#[async_trait]
pub trait MatrixTaskStore: Send {
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
