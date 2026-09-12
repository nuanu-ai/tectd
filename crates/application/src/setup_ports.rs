use async_trait::async_trait;
use tect_domain::{
    FileObservation, FilePublication, NewSetupInput, Result, Setup, SetupContext, SetupDirectory,
    SetupInput,
};
use uuid::Uuid;

/// Synchronous operations stay inside the authorized transaction and row-lock lifetime.
/// Implementations check grant containment before opening paths and pin directory identity.
pub trait SetupFiles: Send + Sync {
    fn resolve_directory(&self, path: &str, current_roots: &[String]) -> Result<SetupDirectory>;
    fn inspect(&self, directory: &SetupDirectory, max_bytes: usize) -> Result<FileObservation>;
    fn publish(&self, directory: &SetupDirectory, content: &str) -> Result<FilePublication>;
}

pub trait SetupOutputGuard: Send + Sync {
    fn input_bytes(&self, input: &str) -> Result<i64>;
    fn check(&self, setup: &Setup) -> Result<()>;
}

/// Setup persistence is part of the SAME UnitOfWork, never an independently committed store.
#[async_trait]
pub trait SetupStore: Send {
    async fn setup_directory(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
    ) -> Result<Option<SetupDirectory>>;
    async fn bind_setup_directory(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
        directory: &SetupDirectory,
    ) -> Result<()>;
    async fn lock_setup_directory(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        path: &str,
    ) -> Result<()>;
    async fn setup_for_directory(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        path: &str,
        for_update: bool,
    ) -> Result<Option<Setup>>;
    async fn setup(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        setup_id: Uuid,
        for_update: bool,
    ) -> Result<Option<Setup>>;
    async fn insert_setup(
        &mut self,
        setup: &Setup,
        session_id: Uuid,
        input: &NewSetupInput,
    ) -> Result<()>;
    async fn update_setup(&mut self, setup: &Setup) -> Result<()>;
    async fn setup_input(
        &mut self,
        workspace_id: Uuid,
        setup_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<SetupInput>>;
    async fn insert_setup_input(
        &mut self,
        workspace_id: Uuid,
        setup_id: Uuid,
        session_id: Uuid,
        sequence: i64,
        input: &NewSetupInput,
    ) -> Result<()>;
    async fn setup_inputs(
        &mut self,
        workspace_id: Uuid,
        setup_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<SetupInput>>;
    async fn setup_context(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
    ) -> Result<Option<SetupContext>>;
}
