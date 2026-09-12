use crate::setups;
use crate::store::PgUnitOfWork;
use async_trait::async_trait;
use tect_application::SetupStore;
use tect_domain::{NewSetupInput, Result, Setup, SetupContext, SetupDirectory, SetupInput};
use uuid::Uuid;

#[async_trait]
impl SetupStore for PgUnitOfWork {
    async fn setup_directory(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
    ) -> Result<Option<SetupDirectory>> {
        let tenant_id = self.tenant_id()?;
        setups::setup_directory(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            session_id,
        )
        .await
    }

    async fn bind_setup_directory(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
        directory: &SetupDirectory,
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        setups::bind_setup_directory(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            session_id,
            directory,
        )
        .await
    }

    async fn lock_setup_directory(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        path: &str,
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        setups::lock_setup_directory(self.transaction()?, tenant_id, workspace_id, host_id, path)
            .await
    }

    async fn setup_for_directory(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        path: &str,
        for_update: bool,
    ) -> Result<Option<Setup>> {
        let tenant_id = self.tenant_id()?;
        setups::setup_for_directory(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            path,
            for_update,
        )
        .await
    }

    async fn setup(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        setup_id: Uuid,
        for_update: bool,
    ) -> Result<Option<Setup>> {
        let tenant_id = self.tenant_id()?;
        setups::setup(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            setup_id,
            for_update,
        )
        .await
    }

    async fn insert_setup(
        &mut self,
        setup: &Setup,
        session_id: Uuid,
        input: &NewSetupInput,
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        setups::insert_setup(self.transaction()?, tenant_id, setup, session_id, input).await
    }

    async fn update_setup(&mut self, setup: &Setup) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        setups::update_setup(self.transaction()?, tenant_id, setup).await
    }

    async fn setup_input(
        &mut self,
        workspace_id: Uuid,
        setup_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<SetupInput>> {
        let tenant_id = self.tenant_id()?;
        setups::setup_input(
            self.transaction()?,
            tenant_id,
            workspace_id,
            setup_id,
            request_id,
        )
        .await
    }

    async fn insert_setup_input(
        &mut self,
        workspace_id: Uuid,
        setup_id: Uuid,
        session_id: Uuid,
        sequence: i64,
        input: &NewSetupInput,
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        setups::insert_setup_input(
            self.transaction()?,
            tenant_id,
            workspace_id,
            setup_id,
            session_id,
            sequence,
            input,
        )
        .await
    }

    async fn setup_inputs(
        &mut self,
        workspace_id: Uuid,
        setup_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<SetupInput>> {
        let tenant_id = self.tenant_id()?;
        setups::setup_inputs(
            self.transaction()?,
            tenant_id,
            workspace_id,
            setup_id,
            after,
            limit,
        )
        .await
    }

    async fn setup_context(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
    ) -> Result<Option<SetupContext>> {
        let tenant_id = self.tenant_id()?;
        setups::setup_context(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            session_id,
        )
        .await
    }
}
