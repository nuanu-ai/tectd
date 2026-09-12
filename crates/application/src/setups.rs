use crate::{SetupOutputGuard, TransactionMode, WorkspaceService};
use tect_domain::{
    Error, FileObservation, NewSetupInput, RequestContext, Result, SaveSetup, Setup,
    SetupDiscovery, SetupFileStatus, SetupPage, setup_path_is_granted, validate_setup_input,
    validate_setup_path,
};
use uuid::Uuid;

impl WorkspaceService {
    /// The native identity is authenticated; launch cwd is explicitly supplied by the agent.
    pub async fn inspect_setup(
        &self,
        context: &RequestContext,
        task_directory: Option<&str>,
        file_capacity: usize,
    ) -> Result<SetupDiscovery> {
        let mut access = self
            .setup_access(context, TransactionMode::ReadWrite)
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
                        access
                            .tx
                            .bind_setup_directory(
                                access.workspace.id,
                                access.identity.host_id,
                                access.session.id,
                                &directory,
                            )
                            .await?;
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

    pub async fn begin_setup(
        &self,
        context: &RequestContext,
        request_id: Uuid,
        input: &str,
        guard: &dyn SetupOutputGuard,
    ) -> Result<Setup> {
        let mut access = self
            .setup_access(context, TransactionMode::ReadWrite)
            .await?;
        validate_setup_input(request_id, input)?;
        let directory = Self::saved_setup_directory(&mut access).await?;
        self.revalidate_setup_directory(&access.identity, &directory)?;
        access
            .tx
            .lock_setup_directory(
                access.workspace.id,
                access.identity.host_id,
                &directory.path,
            )
            .await?;
        if let Some(current) = access
            .tx
            .setup_for_directory(
                access.workspace.id,
                access.identity.host_id,
                &directory.path,
                true,
            )
            .await?
        {
            if current.directory != directory {
                return Err(Error::TaskDirectoryMismatch);
            }
            let original = access
                .tx
                .setup_input(access.workspace.id, current.id, request_id)
                .await?;
            match original {
                Some(original) if original.sequence == 1 && original.input == input => {
                    guard.check(&current)?;
                    access.tx.commit().await?;
                    return Ok(current);
                }
                Some(_) => return Err(Error::InputConflict),
                None => return Err(Error::SetupExists),
            }
        }
        match self.setup_files.inspect(&directory, 0)?.status {
            SetupFileStatus::Missing => {}
            SetupFileStatus::Existing => return Err(Error::SetupFileConflict),
            _ => return Err(Error::SetupUnavailable),
        }
        let input = NewSetupInput {
            request_id,
            input: input.to_owned(),
            encoded_bytes: guard.input_bytes(input)?,
        };
        let setup = Setup::draft(
            Uuid::new_v4(),
            access.workspace.id,
            access.identity.host_id,
            directory,
            input.encoded_bytes,
        );
        guard.check(&setup)?;
        access
            .tx
            .insert_setup(&setup, access.session.id, &input)
            .await?;
        access.tx.commit().await?;
        Ok(setup)
    }

    pub async fn get_setup(
        &self,
        context: &RequestContext,
        setup_id: Uuid,
        after_input: Option<i64>,
        limit: u32,
        file_capacity: usize,
    ) -> Result<SetupPage> {
        let mut access = self
            .setup_access(context, TransactionMode::ReadOnly)
            .await?;
        if !(1..=100).contains(&limit) || after_input.is_some_and(|after| after < 0) {
            return Err(Error::InvalidArguments);
        }
        let setup = self.bound_setup(&mut access, setup_id, false).await?;
        let after = after_input.unwrap_or(setup.input_cursor);
        let mut inputs = access
            .tx
            .setup_inputs(access.workspace.id, setup_id, after, limit + 1)
            .await?;
        let next_after_input = if inputs.len() > limit as usize {
            inputs.pop();
            inputs.last().map(|input| input.sequence)
        } else {
            None
        };
        let file = self.setup_files.inspect(&setup.directory, file_capacity)?;
        access.tx.commit().await?;
        Ok(SetupPage {
            setup,
            inputs,
            next_after_input,
            file,
        })
    }

    pub async fn save_setup(
        &self,
        context: &RequestContext,
        changes: &SaveSetup,
        guard: &dyn SetupOutputGuard,
    ) -> Result<Setup> {
        let mut access = self
            .setup_access(context, TransactionMode::ReadWrite)
            .await?;
        changes.validate()?;
        let current = self
            .bound_setup(&mut access, changes.setup_id, true)
            .await?;
        let setup = current.saved(changes)?;
        guard.check(&setup)?;
        access.tx.update_setup(&setup).await?;
        access.tx.commit().await?;
        Ok(setup)
    }

    pub async fn record_setup_input(
        &self,
        context: &RequestContext,
        setup_id: Uuid,
        revision: i64,
        request_id: Uuid,
        input: &str,
        guard: &dyn SetupOutputGuard,
    ) -> Result<Setup> {
        let mut access = self
            .setup_access(context, TransactionMode::ReadWrite)
            .await?;
        validate_setup_input(request_id, input)?;
        if revision < 1 {
            return Err(Error::InvalidArguments);
        }
        let current = self.bound_setup(&mut access, setup_id, true).await?;
        if let Some(original) = access
            .tx
            .setup_input(access.workspace.id, setup_id, request_id)
            .await?
        {
            if original.input != input {
                return Err(Error::InputConflict);
            }
            guard.check(&current)?;
            access.tx.commit().await?;
            return Ok(current);
        }
        let input = NewSetupInput {
            request_id,
            input: input.to_owned(),
            encoded_bytes: guard.input_bytes(input)?,
        };
        let setup = current.with_new_input(revision, input.encoded_bytes)?;
        guard.check(&setup)?;
        access
            .tx
            .insert_setup_input(
                access.workspace.id,
                setup_id,
                access.session.id,
                setup.latest_input,
                &input,
            )
            .await?;
        access.tx.update_setup(&setup).await?;
        access.tx.commit().await?;
        Ok(setup)
    }
}
