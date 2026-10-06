use crate::{TransactionMode, UnitOfWork, WorkspaceService};
use tect_domain::{Error, PrincipalRole, RequestContext, Result, Workspace};

impl WorkspaceService {
    pub(crate) async fn candidate_read_transaction(
        &self,
        context: &RequestContext,
    ) -> Result<(Box<dyn UnitOfWork>, Workspace, PrincipalRole)> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if !matches!(
            identity.role,
            PrincipalRole::Owner | PrincipalRole::Verifier
        ) {
            return Err(Error::Forbidden);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        Ok((tx, workspace, identity.role))
    }

    /// Authenticate a malformed candidate advisory call without granting the
    /// verifier any of the general workspace state surface.
    pub async fn authenticate_candidate_advisory_session(
        &self,
        context: &RequestContext,
    ) -> Result<()> {
        let (tx, _, _) = self.candidate_read_transaction(context).await?;
        tx.commit().await
    }
}
