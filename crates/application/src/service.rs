use crate::{Store, TransactionMode, UnitOfWork};
use std::sync::Arc;
use tect_domain::{
    Error, EventKind, HostIdentity, RequestContext, Result, Session, Workspace, WorkspaceState,
};

pub struct WorkspaceService {
    store: Arc<dyn Store>,
}

impl WorkspaceService {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
    }

    async fn authorized(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<(Box<dyn UnitOfWork>, HostIdentity)> {
        context.validate()?;
        let mut tx = self.store.begin(mode).await?;
        let identity = tx.authenticate(&context.auth).await?;
        tx.set_tenant(identity.tenant_id).await?;
        Ok((tx, identity))
    }

    async fn validate_binding(
        tx: &mut dyn UnitOfWork,
        context: &RequestContext,
        identity: &HostIdentity,
        session: &Session,
    ) -> Result<Workspace> {
        if session.revoked {
            return Err(Error::SessionRevoked);
        }
        let workspace = tx
            .workspace(session.workspace_id)
            .await?
            .ok_or(Error::Forbidden)?;
        if workspace.key != context.workspace_key {
            return Err(Error::SessionWorkspaceMismatch);
        }
        if !tx.is_member(workspace.id, identity.principal_id).await? {
            return Err(Error::Forbidden);
        }
        Ok(workspace)
    }

    pub async fn get_state(&self, context: &RequestContext) -> Result<WorkspaceState> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let state = match tx
            .session(identity.host_id, &context.native_session_id)
            .await?
        {
            Some(session) => {
                let workspace =
                    Self::validate_binding(&mut *tx, context, &identity, &session).await?;
                WorkspaceState::opened(workspace, session)
            }
            None => WorkspaceState::unopened(),
        };
        tx.commit().await?;
        Ok(state)
    }

    pub async fn open_workspace(&self, context: &RequestContext) -> Result<WorkspaceState> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        if let Some(session) = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
        {
            let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
            tx.commit().await?;
            return Ok(WorkspaceState::opened(workspace, session));
        }
        let workspace = tx.ensure_workspace(&context.workspace_key).await?;
        tx.ensure_membership(workspace.value.id, identity.principal_id)
            .await?;
        let session = tx
            .ensure_session(
                identity.host_id,
                workspace.value.id,
                &context.native_session_id,
            )
            .await?;
        if workspace.created {
            tx.append_creation_event(
                workspace.value.id,
                EventKind::WorkspaceOpened,
                workspace.value.id,
            )
            .await?;
        }
        if session.created {
            tx.append_creation_event(
                workspace.value.id,
                EventKind::SessionOpened,
                session.value.id,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(WorkspaceState::opened(workspace.value, session.value))
    }
}
