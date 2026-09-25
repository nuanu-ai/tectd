use crate::{native_planning, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::NativePlanningStore;
use tect_domain::*;
use sqlx::Row;
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
        let principal = self.principal_id()?;
        let value =
            native_planning::load_context(self.transaction()?, tenant, workspace_id, scope_id)
                .await?;
        if let Some(context) = &value {
            crate::pipeline_execution::authorize_checkpoints(
                self.transaction()?,
                tenant,
                workspace_id,
                principal,
                &context.checkpoints,
            )
            .await?;
        }
        Ok(value)
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
        session_id: Uuid,
        request: &OpenSlice,
    ) -> Result<OpenSliceOutcome> {
        let tenant = self.tenant_id()?;
        let selected = if request.disposition_id.is_some() {
            // The existing receipt must remain replayable after a successful
            // open, when the pre-open recommendation is no longer current.
            let replay: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM native_slices WHERE tenant_id=$1 \
                 AND workspace_id=$2 AND origin_request_id=$3)",
            )
            .bind(tenant)
            .bind(workspace_id)
            .bind(request.request_id)
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(crate::storage_error)?;
            Some(
                crate::pipeline_disposition_store::selection_for_open(
                    self,
                    workspace_id,
                    session_id,
                    request,
                    replay,
                )
                .await?,
            )
        } else {
            None
        };
        native_planning::open_slice(self.transaction()?, tenant, workspace_id, request, selected)
            .await
    }
    async fn slice_open_manifest(
        &mut self,
        workspace_id: Uuid,
        request: &OpenSlice,
    ) -> Result<Option<PipelineRecommendationManifest>> {
        let tenant = self.tenant_id()?;
        let disposition_id = request.disposition_id.ok_or(Error::InvalidArguments)?;
        let replay: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM native_slices WHERE tenant_id=$1 \
             AND workspace_id=$2 AND origin_request_id=$3)",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(request.request_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(crate::storage_error)?;
        if replay {
            return Ok(None);
        }
        let row = sqlx::query(
            "SELECT c.manifest_payload,c.manifest_digest \
             FROM pipeline_advice_dispositions d \
             JOIN pipeline_advice_contexts c ON \
               (c.tenant_id,c.workspace_id,c.opportunity_id)= \
               (d.tenant_id,d.workspace_id,d.opportunity_id) \
             WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.disposition_id=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(disposition_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(crate::storage_error)?
        .ok_or(Error::NotFound)?;
        let manifest: PipelineRecommendationManifest = serde_json::from_value(
            row.try_get::<serde_json::Value, _>("manifest_payload")
                .map_err(crate::storage_error)?,
        )
        .map_err(|_| Error::InputConflict)?;
        if row.try_get::<Option<String>, _>("manifest_digest").map_err(crate::storage_error)?
            .as_deref() != Some(manifest.digest.as_str())
        {
            return Err(Error::InputConflict);
        }
        Ok(Some(manifest))
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
