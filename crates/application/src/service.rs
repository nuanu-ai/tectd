use crate::{SourceInspector, Store, TransactionMode, UnitOfWork};
use std::sync::Arc;
use tect_domain::{
    Error, EventKind, HostIdentity, RequestContext, Result, Session, Workspace, WorkspaceState,
};

pub struct WorkspaceService {
    store: Arc<dyn Store>,
    pub(crate) inspector: Arc<dyn SourceInspector>,
}

impl WorkspaceService {
    pub fn new(store: Arc<dyn Store>, inspector: Arc<dyn SourceInspector>) -> Self {
        Self { store, inspector }
    }

    pub(crate) async fn authorized(
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

    pub(crate) async fn validate_binding(
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

    pub(crate) async fn state(
        tx: &mut dyn UnitOfWork,
        workspace: Workspace,
        session: Session,
    ) -> Result<WorkspaceState> {
        let selected_worktrees = tx
            .selected_worktrees(workspace.id, session.host_id, session.id)
            .await?;
        let mut state = WorkspaceState::opened(workspace, session);
        state.selected_worktrees = selected_worktrees;
        let entries = tx
            .list_programs(state.workspace.as_ref().expect("opened").id, None, 26)
            .await?;
        let page = crate::programs::bounded_program_list(entries, 25);
        state.next_action = Some(
            if page.programs.is_empty() {
                "begin_program"
            } else {
                "get_program"
            }
            .into(),
        );
        state.programs = page.programs;
        state.next_after = page.next_after;
        Ok(state)
    }

    pub(crate) async fn bound_session(
        tx: &mut dyn UnitOfWork,
        context: &RequestContext,
        identity: &HostIdentity,
    ) -> Result<(Workspace, Session)> {
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(tx, context, identity, &session).await?;
        Ok((workspace, session))
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
                Self::state(&mut *tx, workspace, session).await?
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
            let state = Self::state(&mut *tx, workspace, session).await?;
            tx.commit().await?;
            return Ok(state);
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
