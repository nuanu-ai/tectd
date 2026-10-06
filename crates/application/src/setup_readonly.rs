//! Discovery continuation observes the original path without binding a directory.
use crate::{TransactionMode, WorkspaceService};
use tect_domain::{
    Error, FileObservation, RequestContext, Result, SetupDiscovery, setup_path_is_granted,
    validate_setup_path,
};

impl WorkspaceService {
    pub async fn inspect_setup_readonly(
        &self,
        context: &RequestContext,
        task_directory: Option<&str>,
        file_capacity: usize,
    ) -> Result<SetupDiscovery> {
        let mut access = self
            .setup_access(context, TransactionMode::ReadOnly)
            .await?;
        let file = if let Some(path) = task_directory {
            validate_setup_path(path)?;
            let bound = access
                .tx
                .setup_directory(
                    access.workspace.id,
                    access.identity.host_id,
                    access.session.id,
                )
                .await?;
            if bound.as_ref().is_some_and(|saved| saved.path != path) {
                return Err(Error::TaskDirectoryMismatch);
            }
            if !setup_path_is_granted(path, &access.identity.allowed_setup_roots) {
                FileObservation::unavailable("setup_root_not_granted")
            } else {
                match self
                    .setup_files
                    .resolve_directory(path, &access.identity.allowed_setup_roots)
                {
                    Ok(directory) => {
                        if bound.as_ref().is_some_and(|saved| saved != &directory) {
                            return Err(Error::TaskDirectoryMismatch);
                        }
                        if let Some(setup) = access
                            .tx
                            .setup_for_directory(
                                access.workspace.id,
                                access.identity.host_id,
                                path,
                                false,
                            )
                            .await?
                            && setup.directory != directory
                        {
                            return Err(Error::TaskDirectoryMismatch);
                        }
                        self.setup_files.inspect(&directory, file_capacity)?
                    }
                    Err(Error::SetupUnavailable) => {
                        FileObservation::unavailable("task_directory_unavailable")
                    }
                    Err(error) => return Err(error),
                }
            }
        } else {
            FileObservation::unknown()
        };
        let state = Self::state(&mut *access.tx, access.workspace, access.session).await?;
        access.tx.commit().await?;
        Ok(SetupDiscovery { state, file })
    }
}
