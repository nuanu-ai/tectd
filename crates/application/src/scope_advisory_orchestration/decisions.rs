use crate::{ScopeDispositionRecord, TransactionMode, WorkspaceService};
use tect_domain::{RequestContext, Result, ScopeDispositionRequest, ScopeDispositionRevision};
use uuid::Uuid;

impl WorkspaceService {
    pub async fn decide_scope_advisory(
        &self,
        context: &RequestContext,
        opportunity_id: Uuid,
        candidate_set_id: Uuid,
        request: ScopeDispositionRequest,
    ) -> Result<ScopeDispositionRevision> {
        if opportunity_id.is_nil() || candidate_set_id.is_nil() {
            return Err(tect_domain::Error::InvalidArguments);
        }
        let (mut tx, workspace, session) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let actor = tx.session_principal(session.id).await?;
        let result = tx
            .cas_scope_advisory_disposition(
                workspace.id,
                ScopeDispositionRecord {
                    opportunity_id,
                    candidate_set_id,
                    actor_id: actor,
                    session_id: session.id,
                    request,
                },
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }
}
