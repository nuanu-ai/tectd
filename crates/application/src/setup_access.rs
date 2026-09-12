use crate::{TransactionMode, UnitOfWork, WorkspaceService};
use tect_domain::{
    Error, HostIdentity, RequestContext, Result, Session, Setup, SetupDirectory, Workspace,
    setup_path_is_granted,
};
use uuid::Uuid;

pub(crate) struct SetupAccess {
    pub tx: Box<dyn UnitOfWork>,
    pub identity: HostIdentity,
    pub workspace: Workspace,
    pub session: Session,
}

impl WorkspaceService {
    pub(crate) async fn setup_access(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<SetupAccess> {
        let (mut tx, identity) = self.authorized(context, mode).await?;
        if mode == TransactionMode::ReadWrite {
            tx.lock_native_session(identity.host_id, &context.native_session_id)
                .await?;
        }
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        Ok(SetupAccess {
            tx,
            identity,
            workspace,
            session,
        })
    }

    pub(crate) async fn saved_setup_directory(access: &mut SetupAccess) -> Result<SetupDirectory> {
        access
            .tx
            .setup_directory(
                access.workspace.id,
                access.identity.host_id,
                access.session.id,
            )
            .await?
            .ok_or(Error::TaskDirectoryUnbound)
    }

    pub(crate) fn revalidate_setup_directory(
        &self,
        identity: &HostIdentity,
        directory: &SetupDirectory,
    ) -> Result<()> {
        // A binding is identity, never a perpetual capability. Denial precedes even adapter entry.
        if !setup_path_is_granted(&directory.path, &identity.allowed_setup_roots) {
            return Err(Error::SetupUnavailable);
        }
        let current = self
            .setup_files
            .resolve_directory(&directory.path, &identity.allowed_setup_roots)?;
        if &current != directory {
            return Err(Error::TaskDirectoryMismatch);
        }
        Ok(())
    }

    pub(crate) async fn bound_setup(
        &self,
        access: &mut SetupAccess,
        setup_id: Uuid,
        for_update: bool,
    ) -> Result<Setup> {
        if setup_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let binding = Self::saved_setup_directory(access).await?;
        // Check the complete DB resource identity BEFORE touching any filesystem path.
        let setup = access
            .tx
            .setup(
                access.workspace.id,
                access.identity.host_id,
                setup_id,
                for_update,
            )
            .await?
            .ok_or(Error::NotFound)?;
        if setup.directory != binding {
            return Err(Error::NotFound);
        }
        self.revalidate_setup_directory(&access.identity, &binding)?;
        Ok(setup)
    }
}
