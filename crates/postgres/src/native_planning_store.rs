use crate::{native_planning, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::NativePlanningStore;
use tect_domain::*;
use uuid::Uuid;

#[async_trait]
impl NativePlanningStore for PgUnitOfWork {
    async fn native_planning_receipt(
        &mut self,
        workspace_id: Uuid,
        request: &NativePlanningReceiptRequest,
    ) -> Result<Option<SliceCandidateContext>> {
        let tenant = self.tenant_id()?;
        native_planning::planning_receipt(self.transaction()?, tenant, workspace_id, request).await
    }
    async fn native_planning_summaries(
        &mut self,
        workspace_id: Uuid,
        limit: u32,
    ) -> Result<Vec<NativePlanningSummary>> {
        let tenant = self.tenant_id()?;
        native_planning::summaries(self.transaction()?, tenant, workspace_id, limit).await
    }
    async fn scope_open_replay(
        &mut self,
        workspace_id: Uuid,
        request: &OpenScope,
    ) -> Result<Option<OpenScopeOutcome>> {
        let tenant = self.tenant_id()?;
        native_planning::scope_open_replay(self.transaction()?, tenant, workspace_id, request).await
    }
    async fn scope_open_basis(
        &mut self,
        workspace_id: Uuid,
        request: &OpenScope,
    ) -> Result<ScopeOpenBasis> {
        let tenant = self.tenant_id()?;
        native_planning::scope_open_basis(self.transaction()?, tenant, workspace_id, request).await
    }
    async fn open_scope(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &OpenScope,
        material: &SlicePlanningSnapshotMaterial,
    ) -> Result<OpenScopeOutcome> {
        let tenant = self.tenant_id()?;
        native_planning::open_scope(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
            material,
        )
        .await
    }
    async fn native_scope(
        &mut self,
        workspace_id: Uuid,
        scope_id: Uuid,
    ) -> Result<Option<NativeScope>> {
        let tenant = self.tenant_id()?;
        native_planning::load_scope(self.transaction()?, tenant, workspace_id, scope_id).await
    }
    async fn slice_candidate_context(
        &mut self,
        workspace_id: Uuid,
        scope_id: Uuid,
    ) -> Result<Option<SliceCandidateContext>> {
        let tenant = self.tenant_id()?;
        native_planning::load_context(self.transaction()?, tenant, workspace_id, scope_id).await
    }
    async fn save_slice_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        request: &SaveSliceCandidateDraft,
    ) -> Result<SliceCandidateContext> {
        let tenant = self.tenant_id()?;
        native_planning::save_draft(self.transaction()?, tenant, workspace_id, request).await
    }
    async fn review_slice_candidate_set(
        &mut self,
        workspace_id: Uuid,
        request: &ReviewSliceCandidateSet,
    ) -> Result<SliceCandidateContext> {
        let tenant = self.tenant_id()?;
        native_planning::save_review(self.transaction()?, tenant, workspace_id, request).await
    }
    async fn record_slice_candidate_input(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordSliceCandidateInput,
    ) -> Result<SliceCandidateContext> {
        let tenant = self.tenant_id()?;
        native_planning::record_input(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
        )
        .await
    }
    async fn refresh_slice_candidate_set(
        &mut self,
        workspace_id: Uuid,
        request: &RefreshSliceCandidateSet,
        material: &SlicePlanningSnapshotMaterial,
    ) -> Result<SliceCandidateContext> {
        let tenant = self.tenant_id()?;
        native_planning::refresh(self.transaction()?, tenant, workspace_id, request, material).await
    }
    async fn open_slice(
        &mut self,
        workspace_id: Uuid,
        request: &OpenSlice,
    ) -> Result<OpenSliceOutcome> {
        let tenant = self.tenant_id()?;
        native_planning::open_slice(self.transaction()?, tenant, workspace_id, request).await
    }
    async fn native_slice(
        &mut self,
        workspace_id: Uuid,
        slice_id: Uuid,
    ) -> Result<Option<NativeSlice>> {
        let tenant = self.tenant_id()?;
        native_planning::load_slice(self.transaction()?, tenant, workspace_id, slice_id).await
    }
    async fn record_slice_result(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordSliceResult,
    ) -> Result<RecordSliceResultOutcome> {
        let tenant = self.tenant_id()?;
        native_planning::record_result(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
        )
        .await
    }
}
