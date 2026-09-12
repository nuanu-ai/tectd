use crate::{SetupOutputGuard, TransactionMode, WorkspaceService};
use tect_domain::{
    AppliedSetup, Error, FilePublication, PublicationOutcome, RequestContext, Result,
    SetupFileStatus, SetupStatus,
};
use uuid::Uuid;

impl WorkspaceService {
    pub async fn apply_setup(
        &self,
        context: &RequestContext,
        setup_id: Uuid,
        revision: i64,
        guard: &dyn SetupOutputGuard,
    ) -> Result<AppliedSetup> {
        let mut access = self
            .setup_access(context, TransactionMode::ReadWrite)
            .await?;
        let current = self.bound_setup(&mut access, setup_id, true).await?;
        current.validate_apply(revision)?;
        guard.check(&current)?;
        let content = current.content.as_deref().ok_or(Error::SetupIncomplete)?;
        if current.status == SetupStatus::Applied {
            // A historical DB status can never authorize restoration of a removed/changed file.
            let file = self
                .setup_files
                .inspect(&current.directory, content.len())?;
            if file.status != SetupFileStatus::Existing
                || file.byte_length != Some(content.len() as u64)
                || file.sha256.is_none()
                || file.sha256 != current.applied_sha256
            {
                return Err(Error::SetupFileConflict);
            }
            let publication = FilePublication {
                outcome: PublicationOutcome::AlreadyMatches,
                sha256: file.sha256.expect("verified above"),
                byte_length: content.len() as u64,
            };
            access.tx.commit().await?;
            return Ok(AppliedSetup {
                setup: current,
                publication,
            });
        }
        // Synchronous publication stays within current auth/row locks. SQL rollback cannot undo it.
        // A failure after this line leaves durable ready intent; retry verifies matching bytes.
        let publication = self.setup_files.publish(&current.directory, content)?;
        if publication.byte_length != content.len() as u64 {
            return Err(Error::SetupFileConflict);
        }
        let setup = current.applied(revision, &publication.sha256)?;
        access.tx.update_setup(&setup).await?;
        access.tx.commit().await?;
        Ok(AppliedSetup { setup, publication })
    }
}
