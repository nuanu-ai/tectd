use crate::{scope_candidates, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::ScopeCandidateStore;
use tect_domain::{
    BeginCandidateSet, BeginCandidateSetOutcome, CandidateHistoryEntry, CandidateInputSummary,
    CandidateReceiptRequest, CandidateSetSummary, CandidateSnapshotMaterial, CandidateTextFragment,
    RecordCandidateInput, RefreshCandidateSet, Result, ReviewCandidateSet, SaveCandidateDraft,
    StoredCandidateContext, StoredHistoricalCandidateDraft,
};
use uuid::Uuid;

#[async_trait]
impl ScopeCandidateStore for PgUnitOfWork {
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
        let tenant_id = self.tenant_id()?;
        scope_candidates::save_draft(self.transaction()?, tenant_id, workspace_id, request).await
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
