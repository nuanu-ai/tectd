use crate::{TransactionMode, WorkspaceService};
use tect_domain::{
    Error, MAX_SOURCE_PATH_BYTES, MAX_WORKTREES, RegisteredSource, RequestContext, Result,
    SourcePage, WorkspaceState, validate_selection,
};
use uuid::Uuid;

impl WorkspaceService {
    pub async fn register_source(
        &self,
        context: &RequestContext,
        path: &str,
    ) -> Result<RegisteredSource> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        if path.is_empty() || path.len() > MAX_SOURCE_PATH_BYTES || path.contains('\0') {
            return Err(Error::InvalidSource);
        }
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let location = self
            .inspector
            .inspect(path, &identity.allowed_source_roots)
            .await?;
        if location.common_dir.len() > MAX_SOURCE_PATH_BYTES
            || location.worktree_path.len() > MAX_SOURCE_PATH_BYTES
        {
            return Err(Error::InvalidSource);
        }
        let source = tx
            .register_source(workspace.id, identity.host_id, &location)
            .await?;
        tx.commit().await?;
        Ok(source)
    }

    pub async fn select_worktrees(
        &self,
        context: &RequestContext,
        ids: &[Uuid],
    ) -> Result<WorkspaceState> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        validate_selection(ids)?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let selected = tx
            .source_worktrees(workspace.id, identity.host_id, ids)
            .await?;
        if selected.len() != ids.len() || selected.iter().any(|s| !ids.contains(&s.id)) {
            return Err(Error::InvalidWorktreeSelection);
        }
        tx.replace_selection(workspace.id, identity.host_id, session.id, ids)
            .await?;
        let state = Self::state(&mut *tx, workspace, session).await?;
        tx.commit().await?;
        Ok(state)
    }

    pub async fn list_sources(
        &self,
        context: &RequestContext,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<SourcePage> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        if !(1..=MAX_WORKTREES as u32).contains(&limit) || after.is_some_and(|id| id.is_nil()) {
            return Err(Error::InvalidArguments);
        }
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let mut items = tx
            .list_sources(workspace.id, identity.host_id, after, limit + 1)
            .await?;
        let has_more = items.len() > limit as usize;
        items.truncate(limit as usize);
        let next_after = if has_more {
            items.last().map(|s| s.id)
        } else {
            None
        };
        tx.commit().await?;
        Ok(SourcePage { items, next_after })
    }
}
