use async_trait::async_trait;
use tect_domain::{
    Created, EventKind, HostAuth, HostIdentity, RegisteredSource, Result, Session, SourceLocation,
    Workspace, WorktreeSummary,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionMode {
    ReadOnly,
    ReadWrite,
}

#[async_trait]
pub trait Store: Send + Sync {
    async fn begin(&self, mode: TransactionMode) -> Result<Box<dyn UnitOfWork>>;
}

/// A dropped unit of work rolls back. No database-specific types escape this port.
#[async_trait]
pub trait UnitOfWork: Send {
    async fn authenticate(&mut self, auth: &HostAuth) -> Result<HostIdentity>;
    async fn set_tenant(&mut self, tenant_id: Uuid) -> Result<()>;
    async fn lock_native_session(&mut self, host_id: Uuid, native_id: &str) -> Result<()>;
    async fn session(&mut self, host_id: Uuid, native_id: &str) -> Result<Option<Session>>;
    async fn workspace(&mut self, id: Uuid) -> Result<Option<Workspace>>;
    async fn is_member(&mut self, workspace_id: Uuid, principal_id: Uuid) -> Result<bool>;
    async fn ensure_workspace(&mut self, key: &str) -> Result<Created<Workspace>>;
    async fn ensure_membership(&mut self, workspace_id: Uuid, principal_id: Uuid) -> Result<()>;
    async fn ensure_session(
        &mut self,
        host_id: Uuid,
        workspace_id: Uuid,
        native_id: &str,
    ) -> Result<Created<Session>>;
    async fn append_creation_event(
        &mut self,
        workspace_id: Uuid,
        kind: EventKind,
        entity_id: Uuid,
    ) -> Result<()>;
    async fn register_source(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        location: &SourceLocation,
    ) -> Result<RegisteredSource>;
    async fn source_worktrees(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<WorktreeSummary>>;
    async fn replace_selection(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
        ids: &[Uuid],
    ) -> Result<()>;
    async fn selected_worktrees(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
    ) -> Result<Vec<WorktreeSummary>>;
    /// Return at most limit entries, in UUID order; limit may be 101 for look-ahead.
    async fn list_sources(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<RegisteredSource>>;
    async fn commit(self: Box<Self>) -> Result<()>;
}

/// Host adapter validates real Git paths without mutating repositories.
#[async_trait]
pub trait SourceInspector: Send + Sync {
    async fn inspect(&self, path: &str, allowed_roots: &[String]) -> Result<SourceLocation>;
}
