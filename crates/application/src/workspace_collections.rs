use crate::{TransactionMode, WorkspaceService};
use tect_domain::{
    CandidateSetList, Error, NativePlanningList, RequestContext, Result, Session, Workspace,
    WorkspaceCollection, WorkspaceCollectionCursor,
};

fn cursor(
    after: Option<&str>,
    limit: u32,
    workspace: &Workspace,
    collection: WorkspaceCollection,
) -> Result<Option<WorkspaceCollectionCursor>> {
    if !(1..=25).contains(&limit) {
        return Err(Error::InvalidArguments);
    }
    let cursor = after.map(WorkspaceCollectionCursor::parse).transpose()?;
    if let Some(cursor) = cursor {
        cursor.validate(workspace.id, collection)?;
    }
    Ok(cursor)
}

impl WorkspaceService {
    pub async fn read_candidate_sets_bound(
        &self,
        context: &RequestContext,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Workspace, Session, CandidateSetList)> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let after = cursor(after, limit, &workspace, WorkspaceCollection::CandidateSets)?;
        let page = tx.candidate_heads(workspace.id, after, limit).await?;
        tx.commit().await?;
        Ok((workspace, session, page))
    }

    pub async fn read_native_planning_bound(
        &self,
        context: &RequestContext,
        after: Option<&str>,
        limit: u32,
    ) -> Result<(Workspace, Session, NativePlanningList)> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let after = cursor(
            after,
            limit,
            &workspace,
            WorkspaceCollection::NativePlanning,
        )?;
        let page = tx
            .native_planning_summaries(workspace.id, after, limit)
            .await?;
        tx.commit().await?;
        Ok((workspace, session, page))
    }
}
