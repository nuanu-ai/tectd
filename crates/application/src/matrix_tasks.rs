use crate::{TransactionMode, WorkspaceService};
use sha2::{Digest, Sha256};
use tect_domain::{EngineeringMatrixInput, Error, RequestContext, Result};
use uuid::Uuid;

pub const MATRIX_INPUT_SCHEMA: &str = "tect.engineering-matrix-input/1";

/// The requested revision is exact: a new task starts at 1 and each edit
/// must name the immediate successor of the accepted revision.
#[derive(Debug, Clone)]
pub struct RecordMatrixTask {
    pub task_id: Uuid,
    pub revision: i64,
    pub expected_current_revision: i64,
    pub request_id: Uuid,
    pub input: EngineeringMatrixInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixTaskRevision {
    pub task_id: Uuid,
    pub revision: i64,
    pub request_id: Uuid,
    pub input: EngineeringMatrixInput,
    pub input_digest: String,
    pub recorded_by_principal_id: Uuid,
    pub recorded_by_session_id: Uuid,
}

impl WorkspaceService {
    pub async fn record_matrix_task(
        &self,
        context: &RequestContext,
        request: &RecordMatrixTask,
    ) -> Result<MatrixTaskRevision> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        validate_request(request)?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let encoded = serde_json::to_vec(&request.input).map_err(|_| Error::InvalidArguments)?;
        let input_digest = format!("{:x}", Sha256::digest(encoded));
        let revision = tx
            .record_matrix_task(
                workspace.id,
                identity.principal_id,
                session.id,
                request,
                &input_digest,
            )
            .await?;
        tx.commit().await?;
        Ok(revision)
    }

    pub async fn get_matrix_task(
        &self,
        context: &RequestContext,
        task_id: Uuid,
    ) -> Result<MatrixTaskRevision> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if task_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let revision = tx
            .matrix_task(workspace.id, task_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.commit().await?;
        Ok(revision)
    }
}

fn validate_request(request: &RecordMatrixTask) -> Result<()> {
    if request.task_id.is_nil()
        || request.request_id.is_nil()
        || request.revision < 1
        || request.expected_current_revision != request.revision - 1
    {
        return Err(Error::InvalidArguments);
    }
    request.input.validate()
}
