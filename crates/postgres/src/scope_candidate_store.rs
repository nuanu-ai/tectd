use crate::{scope_candidates, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::{CandidateDeltaStore, ScopeCandidateStore};
use tect_domain::{
    AdvisoryOpportunity, BeginCandidateSet, BeginCandidateSetOutcome, CandidateHistoryEntry,
    CandidateInputSummary, CandidateReceiptRequest, CandidateSetSummary, CandidateSnapshotMaterial,
    CandidateTextFragment, RecordCandidateInput, RefreshCandidateSet, Result, ReviewCandidateSet,
    SaveCandidateDraft, StoredCandidateContext, StoredHistoricalCandidateDraft,
};
use uuid::Uuid;

#[async_trait]
impl ScopeCandidateStore for PgUnitOfWork {
    async fn matrix_decomposition_parent(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<AdvisoryOpportunity>> {
        let tenant_id = self.tenant_id()?;
        crate::advisory::matrix_decomposition_parent(
            self.transaction()?,
            tenant_id,
            workspace_id,
            opportunity_id,
        )
        .await
    }

    async fn candidate_begin_replay(
        &mut self,
        workspace_id: Uuid,
        request: &BeginCandidateSet,
    ) -> Result<Option<BeginCandidateSetOutcome>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::begin_replay(self.transaction()?, tenant_id, workspace_id, request).await
    }

    async fn candidate_receipt(
        &mut self,
        workspace_id: Uuid,
        request: &CandidateReceiptRequest,
    ) -> Result<Option<StoredCandidateContext>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::replay(self.transaction()?, tenant_id, workspace_id, request).await
    }
    async fn selected_candidate_receipt(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        session_id: Uuid,
        request: &SaveCandidateDraft,
    ) -> Result<Option<StoredCandidateContext>> {
        let tenant_id = self.tenant_id()?;
        crate::scope_advisory::selected_candidate_receipt(
            self.transaction()?,
            tenant_id,
            workspace_id,
            actor_id,
            session_id,
            request,
        )
        .await
    }
    async fn ensure_candidate_set(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &BeginCandidateSet,
        input_bytes: i64,
        material: &CandidateSnapshotMaterial,
    ) -> Result<BeginCandidateSetOutcome> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::ensure(
            self.transaction()?,
            tenant_id,
            workspace_id,
            session_id,
            request,
            input_bytes,
            material,
        )
        .await
    }

    async fn candidate_context(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<Option<StoredCandidateContext>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::load(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
        )
        .await
    }

    async fn candidate_revision(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<Option<i64>> {
        let tenant_id = self.tenant_id()?;
        sqlx::query_scalar(
            "SELECT revision FROM scope_candidate_sets \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(crate::storage_error)
    }

    async fn lock_candidate_revision(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<Option<i64>> {
        let tenant_id = self.tenant_id()?;
        sqlx::query_scalar(
            "SELECT revision FROM scope_candidate_sets \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR SHARE",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(crate::storage_error)
    }

    async fn candidate_history(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<CandidateHistoryEntry>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::history(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
            after,
            limit,
        )
        .await
    }

    async fn historical_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        draft_revision: i64,
    ) -> Result<Option<StoredHistoricalCandidateDraft>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::historical(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
            draft_revision,
        )
        .await
    }

    async fn candidate_heads(
        &mut self,
        workspace_id: Uuid,
        limit: u32,
    ) -> Result<Vec<CandidateSetSummary>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::heads(self.transaction()?, tenant_id, workspace_id, limit).await
    }

    async fn candidate_inputs(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<CandidateInputSummary>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::input_summaries(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
            after,
            limit,
        )
        .await
    }

    async fn candidate_fragment(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        snapshot_id: Option<Uuid>,
        source_ref_id: Uuid,
        cursor: usize,
        max_bytes: usize,
    ) -> Result<CandidateTextFragment> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::fragment(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
            snapshot_id,
            source_ref_id,
            cursor,
            max_bytes,
        )
        .await
    }

    async fn save_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        request: &SaveCandidateDraft,
    ) -> Result<StoredCandidateContext> {
        if request.selected_advisory.is_some() {
            return Err(tect_domain::Error::InvalidArguments);
        }
        let tenant_id = self.tenant_id()?;
        scope_candidates::save_draft(self.transaction()?, tenant_id, workspace_id, request).await
    }

    async fn save_selected_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        session_id: Uuid,
        request: &SaveCandidateDraft,
    ) -> Result<StoredCandidateContext> {
        let tenant_id = self.tenant_id()?;
        crate::scope_advisory::save_selected_candidate_draft(
            self.transaction()?,
            tenant_id,
            workspace_id,
            actor_id,
            session_id,
            request,
        )
        .await
    }

    async fn save_candidate_review(
        &mut self,
        workspace_id: Uuid,
        request: &ReviewCandidateSet,
    ) -> Result<StoredCandidateContext> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::save_review(self.transaction()?, tenant_id, workspace_id, request).await
    }

    async fn record_candidate_input(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordCandidateInput,
        input_bytes: i64,
    ) -> Result<StoredCandidateContext> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::record_input(
            self.transaction()?,
            tenant_id,
            workspace_id,
            session_id,
            request,
            input_bytes,
        )
        .await
    }

    async fn refresh_candidate_set(
        &mut self,
        workspace_id: Uuid,
        request: &RefreshCandidateSet,
        material: &CandidateSnapshotMaterial,
    ) -> Result<StoredCandidateContext> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::refresh(
            self.transaction()?,
            tenant_id,
            workspace_id,
            request,
            material,
        )
        .await
    }
}

include!("scope_candidate_store/delta.rs");

async fn ensure_source_ref(
    connection: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    source_ref_id: Uuid,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scope_candidate_source_refs
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND id=$4)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(source_ref_id)
    .fetch_one(connection)
    .await
    .map_err(crate::storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(tect_domain::Error::NotFound)
    }
}

async fn ensure_live_candidate(
    connection: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    candidate_id: Uuid,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scope_candidate_delta_candidates
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4 AND NOT deleted)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(candidate_id)
    .fetch_one(connection)
    .await
    .map_err(crate::storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(tect_domain::Error::NotFound)
    }
}

async fn ensure_live_goal(
    connection: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    goal_id: Uuid,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scope_candidate_delta_goals
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND goal_id=$4 AND NOT deleted)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(goal_id)
    .fetch_one(connection)
    .await
    .map_err(crate::storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(tect_domain::Error::NotFound)
    }
}
