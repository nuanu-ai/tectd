use async_trait::async_trait;
use tect_domain::{
    BeginCandidateSet, BeginCandidateSetOutcome, CandidateHistoryEntry, CandidateInputSummary,
    CandidateReceiptRequest, CandidateSetSummary, CandidateSnapshotMaterial, CandidateTextFragment,
    Program, RecordCandidateInput, RefreshCandidateSet, ResolvedCandidateDraft, Result,
    ReviewCandidateSet, SaveCandidateDraft, StoredCandidateContext, StoredHistoricalCandidateDraft,
    WorktreeSummary,
};
use uuid::Uuid;

pub trait CandidateGuidance: Send + Sync {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial>;
}

pub trait CandidateOutputGuard: Send + Sync {
    fn input_bytes(&self, input: &str) -> Result<i64>;
    fn check_material(&self, material: &CandidateSnapshotMaterial) -> Result<()>;
    fn check_draft(&self, draft: &ResolvedCandidateDraft) -> Result<()>;
    fn check_stored(&self, stored: &StoredCandidateContext) -> Result<()>;
    fn check_begin(&self, outcome: &BeginCandidateSetOutcome) -> Result<()>;
}

#[async_trait]
pub trait ScopeCandidateStore: Send {
    async fn candidate_begin_replay(
        &mut self,
        workspace_id: Uuid,
        request: &BeginCandidateSet,
    ) -> Result<Option<BeginCandidateSetOutcome>>;
    async fn candidate_receipt(
        &mut self,
        workspace_id: Uuid,
        request: &CandidateReceiptRequest,
    ) -> Result<Option<StoredCandidateContext>>;
    async fn selected_candidate_receipt(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        session_id: Uuid,
        request: &SaveCandidateDraft,
    ) -> Result<Option<StoredCandidateContext>> {
        let _ = (workspace_id, actor_id, session_id, request);
        Err(tect_domain::Error::Forbidden)
    }
    async fn ensure_candidate_set(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &BeginCandidateSet,
        input_bytes: i64,
        material: &CandidateSnapshotMaterial,
    ) -> Result<BeginCandidateSetOutcome>;
    async fn candidate_context(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<Option<StoredCandidateContext>>;
    /// Tenant/workspace-scoped target lookup without loading source material.
    async fn candidate_revision(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<Option<i64>>;
    /// Lock the target row through the write transaction until opportunity capture commits.
    async fn lock_candidate_revision(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<Option<i64>>;
    async fn candidate_heads(
        &mut self,
        workspace_id: Uuid,
        limit: u32,
    ) -> Result<Vec<CandidateSetSummary>>;
    async fn candidate_inputs(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<CandidateInputSummary>>;
    async fn candidate_fragment(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        snapshot_id: Option<Uuid>,
        source_ref_id: Uuid,
        cursor: usize,
        max_bytes: usize,
    ) -> Result<CandidateTextFragment>;
    async fn candidate_history(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<CandidateHistoryEntry>>;
    async fn historical_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        draft_revision: i64,
    ) -> Result<Option<StoredHistoricalCandidateDraft>>;
    async fn save_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        request: &SaveCandidateDraft,
    ) -> Result<StoredCandidateContext>;
    async fn save_selected_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        session_id: Uuid,
        request: &SaveCandidateDraft,
    ) -> Result<StoredCandidateContext> {
        let _ = (workspace_id, actor_id, session_id, request);
        Err(tect_domain::Error::Forbidden)
    }
    async fn save_candidate_review(
        &mut self,
        workspace_id: Uuid,
        request: &ReviewCandidateSet,
    ) -> Result<StoredCandidateContext>;
    async fn record_candidate_input(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordCandidateInput,
        input_bytes: i64,
    ) -> Result<StoredCandidateContext>;
    async fn refresh_candidate_set(
        &mut self,
        workspace_id: Uuid,
        request: &RefreshCandidateSet,
        material: &CandidateSnapshotMaterial,
    ) -> Result<StoredCandidateContext>;
}
