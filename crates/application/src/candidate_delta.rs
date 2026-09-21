use crate::{TransactionMode, WorkspaceService};
use tect_domain::{CandidateDeltaBatch, CandidateDeltaReceipt, Error, RequestContext, Result};
use uuid::Uuid;

impl WorkspaceService {
    pub async fn apply_candidate_delta(
        &self,
        context: &RequestContext,
        request: &CandidateDeltaBatch,
    ) -> Result<CandidateDeltaReceipt> {
        request.validate()?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let result = tx.apply_candidate_delta(workspace.id, request).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn candidate_delta_status(
        &self,
        context: &RequestContext,
        candidate_set_id: Uuid,
        idempotency_key: &str,
    ) -> Result<CandidateDeltaReceipt> {
        if candidate_set_id.is_nil() || idempotency_key.is_empty() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let result = tx
            .candidate_delta_status(workspace.id, candidate_set_id, idempotency_key)
            .await?
            .ok_or(Error::NotFound)?;
        tx.commit().await?;
        Ok(result)
    }
}
