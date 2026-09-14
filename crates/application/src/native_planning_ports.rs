use async_trait::async_trait;
use tect_domain::{
    NativePlanningReceiptRequest, NativePlanningSummary, NativeScope, NativeSlice, OpenScope,
    OpenScopeOutcome, OpenSlice, OpenSliceOutcome, RecordSliceCandidateInput, RecordSliceResult,
    RecordSliceResultOutcome, RefreshSliceCandidateSet, Result, ReviewSliceCandidateSet,
    SaveSliceCandidateDraft, ScopeOpenBasis, SliceCandidateContext, SlicePlanningSnapshotMaterial,
    SliceResult,
};
use uuid::Uuid;

pub trait NativePlanningGuidance: Send + Sync {
    /// Host-owned method/rule/catalogue bodies. Core persists an immutable copy.
    fn snapshot(
        &self,
        basis: &ScopeOpenBasis,
        inputs: &[tect_domain::SlicePlanningInput],
        results: &[SliceResult],
    ) -> Result<SlicePlanningSnapshotMaterial>;
}

pub trait NativePlanningOutputGuard: Send + Sync {
    fn check_context(&self, value: &SliceCandidateContext) -> Result<()>;
    fn check_open_scope(&self, value: &OpenScopeOutcome) -> Result<()>;
}

#[async_trait]
pub trait NativePlanningStore: Send {
    async fn native_planning_receipt(
        &mut self,
        workspace_id: Uuid,
        request: &NativePlanningReceiptRequest,
    ) -> Result<Option<SliceCandidateContext>>;
    async fn native_planning_summaries(
        &mut self,
        workspace_id: Uuid,
        limit: u32,
    ) -> Result<Vec<NativePlanningSummary>>;
    async fn scope_open_replay(
        &mut self,
        workspace_id: Uuid,
        request: &OpenScope,
    ) -> Result<Option<OpenScopeOutcome>>;
    async fn scope_open_basis(
        &mut self,
        workspace_id: Uuid,
        request: &OpenScope,
    ) -> Result<ScopeOpenBasis>;
    async fn open_scope(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &OpenScope,
        material: &SlicePlanningSnapshotMaterial,
    ) -> Result<OpenScopeOutcome>;
    async fn native_scope(
        &mut self,
        workspace_id: Uuid,
        scope_id: Uuid,
    ) -> Result<Option<NativeScope>>;
    async fn slice_candidate_context(
        &mut self,
        workspace_id: Uuid,
        scope_id: Uuid,
    ) -> Result<Option<SliceCandidateContext>>;
    async fn save_slice_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        request: &SaveSliceCandidateDraft,
    ) -> Result<SliceCandidateContext>;
    async fn review_slice_candidate_set(
        &mut self,
        workspace_id: Uuid,
        request: &ReviewSliceCandidateSet,
    ) -> Result<SliceCandidateContext>;
    async fn record_slice_candidate_input(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordSliceCandidateInput,
    ) -> Result<SliceCandidateContext>;
    async fn refresh_slice_candidate_set(
        &mut self,
        workspace_id: Uuid,
        request: &RefreshSliceCandidateSet,
        material: &SlicePlanningSnapshotMaterial,
    ) -> Result<SliceCandidateContext>;
    async fn open_slice(
        &mut self,
        workspace_id: Uuid,
        request: &OpenSlice,
    ) -> Result<OpenSliceOutcome>;
    async fn native_slice(
        &mut self,
        workspace_id: Uuid,
        slice_id: Uuid,
    ) -> Result<Option<NativeSlice>>;
    async fn record_slice_result(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordSliceResult,
    ) -> Result<RecordSliceResultOutcome>;
}
