use crate::{
    CandidateGuidance, NativePlanningGuidance, TransactionMode, UnitOfWork, WorkspaceService,
};
use tect_domain::{
    Error, NativeScope, NativeSlice, OpenScope, OpenScopeOutcome, OpenSlice, OpenSliceOutcome,
    RecordSliceCandidateInput, RecordSliceResult, RecordSliceResultOutcome,
    RefreshSliceCandidateSet, Result, ReviewSliceCandidateSet, SaveSliceCandidateDraft,
    SliceCandidateContext, SliceCandidateContextQuery,
};

impl WorkspaceService {
    pub(crate) async fn native_planning_transaction(
        &self,
        context: &tect_domain::RequestContext,
        mode: TransactionMode,
    ) -> Result<(
        Box<dyn UnitOfWork>,
        tect_domain::Workspace,
        tect_domain::Session,
    )> {
        let (mut tx, identity) = self.authorized(context, mode).await?;
        if mode == TransactionMode::ReadWrite {
            tx.lock_native_session(identity.host_id, &context.native_session_id)
                .await?;
        }
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        Ok((tx, workspace, session))
    }

    pub async fn scope_open(
        &self,
        context: &tect_domain::RequestContext,
        request: &OpenScope,
        source_guidance: &dyn CandidateGuidance,
        slice_guidance: &dyn NativePlanningGuidance,
    ) -> Result<OpenScopeOutcome> {
        validate_scope_open(request)?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        if let Some(replay) = tx.scope_open_replay(workspace.id, request).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        let basis = tx.scope_open_basis(workspace.id, request).await?;
        crate::scope_candidates::ensure_fresh(
            &mut *tx,
            workspace.id,
            session.host_id,
            session.id,
            request.candidate_set_id,
            source_guidance,
        )
        .await?;
        let material = slice_guidance.snapshot(&basis, &[], &[])?;
        validate_material(&material)?;
        let outcome = tx
            .open_scope(workspace.id, session.id, request, &material)
            .await?;
        tx.commit().await?;
        Ok(outcome)
    }

    pub async fn scope_context(
        &self,
        context: &tect_domain::RequestContext,
        scope_id: uuid::Uuid,
    ) -> Result<NativeScope> {
        if scope_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let value = tx
            .native_scope(workspace.id, scope_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn slice_candidate_context(
        &self,
        context: &tect_domain::RequestContext,
        query: &SliceCandidateContextQuery,
        guidance: &dyn NativePlanningGuidance,
    ) -> Result<SliceCandidateContext> {
        if query.scope_id.is_nil() || query.limit == 0 || query.limit > 100 {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let mut stored = tx
            .slice_candidate_context(workspace.id, query.scope_id)
            .await?
            .ok_or(Error::NotFound)?;
        let basis = basis_from_context(&stored)?;
        let current = guidance.snapshot(&basis, &stored.inputs, &stored.results)?;
        stored.stale_reasons = slice_stale_reasons(&stored, &current);
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn save_slice_candidate_draft(
        &self,
        context: &tect_domain::RequestContext,
        request: &SaveSliceCandidateDraft,
        guidance: &dyn NativePlanningGuidance,
    ) -> Result<SliceCandidateContext> {
        request.draft.validate()?;
        validate_slice_write(
            request.scope_id,
            request.candidate_set_id,
            request.revision,
            request.snapshot_id,
            request.input_cursor,
            request.request_id,
        )?;
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::NativePlanningReceiptRequest::SaveDraft(request.clone());
        if let Some(value) = tx.native_planning_receipt(workspace.id, &receipt).await? {
            tx.commit().await?;
            return Ok(value);
        }
        ensure_slice_fresh(&mut *tx, workspace.id, request.scope_id, guidance).await?;
        let value = tx.save_slice_candidate_draft(workspace.id, request).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn review_slice_candidate_set(
        &self,
        context: &tect_domain::RequestContext,
        request: &ReviewSliceCandidateSet,
        guidance: &dyn NativePlanningGuidance,
    ) -> Result<SliceCandidateContext> {
        validate_slice_write(
            request.scope_id,
            request.candidate_set_id,
            request.revision,
            request.snapshot_id,
            request.input_cursor,
            request.request_id,
        )?;
        if request.review.summary.trim().is_empty() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::NativePlanningReceiptRequest::Review(request.clone());
        if let Some(value) = tx.native_planning_receipt(workspace.id, &receipt).await? {
            tx.commit().await?;
            return Ok(value);
        }
        ensure_slice_fresh(&mut *tx, workspace.id, request.scope_id, guidance).await?;
        let value = tx.review_slice_candidate_set(workspace.id, request).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn record_slice_candidate_input(
        &self,
        context: &tect_domain::RequestContext,
        request: &RecordSliceCandidateInput,
    ) -> Result<SliceCandidateContext> {
        if request.scope_id.is_nil()
            || request.candidate_set_id.is_nil()
            || request.request_id.is_nil()
            || request.revision < 1
            || request.input.trim().is_empty()
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::NativePlanningReceiptRequest::RecordInput(request.clone());
        if let Some(mut value) = tx.native_planning_receipt(workspace.id, &receipt).await? {
            value.stale_reasons = vec!["planning_inputs".into()];
            tx.commit().await?;
            return Ok(value);
        }
        let mut value = tx
            .record_slice_candidate_input(workspace.id, session.id, request)
            .await?;
        value.stale_reasons = vec!["planning_inputs".into()];
        tx.commit().await?;
        Ok(value)
    }

    pub async fn refresh_slice_candidate_set(
        &self,
        context: &tect_domain::RequestContext,
        request: &RefreshSliceCandidateSet,
        guidance: &dyn NativePlanningGuidance,
    ) -> Result<SliceCandidateContext> {
        if request.scope_id.is_nil()
            || request.candidate_set_id.is_nil()
            || request.request_id.is_nil()
            || request.revision < 1
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::NativePlanningReceiptRequest::Refresh(request.clone());
        if let Some(value) = tx.native_planning_receipt(workspace.id, &receipt).await? {
            tx.commit().await?;
            return Ok(value);
        }
        let stored = tx
            .slice_candidate_context(workspace.id, request.scope_id)
            .await?
            .ok_or(Error::NotFound)?;
        let basis = basis_from_context(&stored)?;
        let material = guidance.snapshot(&basis, &stored.inputs, &stored.results)?;
        validate_material(&material)?;
        let value = tx
            .refresh_slice_candidate_set(workspace.id, request, &material)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn slice_open(
        &self,
        context: &tect_domain::RequestContext,
        request: &OpenSlice,
    ) -> Result<OpenSliceOutcome> {
        if request.request_id.is_nil()
            || request.scope_id.is_nil()
            || request.scope_revision < 1
            || request.candidate_set_id.is_nil()
            || request.candidate_set_revision < 1
            || request.candidate_snapshot_id.is_nil()
            || request.candidate_id.is_nil()
            || request.candidate_revision < 1
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx.open_slice(workspace.id, request).await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn slice_context(
        &self,
        context: &tect_domain::RequestContext,
        slice_id: uuid::Uuid,
    ) -> Result<NativeSlice> {
        if slice_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let value = tx
            .native_slice(workspace.id, slice_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn slice_result_record(
        &self,
        context: &tect_domain::RequestContext,
        request: &RecordSliceResult,
    ) -> Result<RecordSliceResultOutcome> {
        validate_result(request)?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx
            .record_slice_result(workspace.id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }
}

async fn ensure_slice_fresh(
    tx: &mut dyn UnitOfWork,
    workspace_id: uuid::Uuid,
    scope_id: uuid::Uuid,
    guidance: &dyn NativePlanningGuidance,
) -> Result<()> {
    let stored = tx
        .slice_candidate_context(workspace_id, scope_id)
        .await?
        .ok_or(Error::NotFound)?;
    let current = guidance.snapshot(
        &basis_from_context(&stored)?,
        &stored.inputs,
        &stored.results,
    )?;
    if slice_stale_reasons(&stored, &current).is_empty() {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

fn basis_from_context(context: &SliceCandidateContext) -> Result<tect_domain::ScopeOpenBasis> {
    Ok(tect_domain::ScopeOpenBasis {
        boundary: context.scope.boundary,
        title: context.scope.title.clone(),
        outcome: context.scope.outcome.clone(),
        includes: context.scope.includes.clone(),
        excludes: context.scope.excludes.clone(),
        source_candidate_set_revision: context.snapshot.source_candidate_set_revision,
        source_snapshot_id: context.snapshot.source_snapshot_id,
    })
}

fn validate_scope_open(r: &OpenScope) -> Result<()> {
    if r.request_id.is_nil()
        || r.candidate_set_id.is_nil()
        || r.candidate_set_revision < 1
        || r.candidate_snapshot_id.is_nil()
        || r.candidate_id.is_nil()
        || r.candidate_revision < 1
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
fn validate_slice_write(
    scope: uuid::Uuid,
    set: uuid::Uuid,
    revision: i64,
    snapshot: uuid::Uuid,
    cursor: i64,
    request: uuid::Uuid,
) -> Result<()> {
    if scope.is_nil()
        || set.is_nil()
        || snapshot.is_nil()
        || request.is_nil()
        || revision < 1
        || cursor < 0
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
fn validate_material(m: &tect_domain::SlicePlanningSnapshotMaterial) -> Result<()> {
    m.catalogue.validate()?;
    if m.method.id.trim().is_empty()
        || m.method.revision.trim().is_empty()
        || m.method.digest.trim().is_empty()
        || m.method.body.trim().is_empty()
        || m.registry_revision.trim().is_empty()
        || m.registry_digest.trim().is_empty()
        || m.rules.len() != 4
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
fn validate_result(r: &RecordSliceResult) -> Result<()> {
    if r.request_id.is_nil()
        || r.scope_id.is_nil()
        || r.slice_id.is_nil()
        || r.slice_revision < 1
        || r.summary.trim().is_empty()
        || r.evidence.is_empty()
        || r.evidence.iter().any(|e| {
            e.kind.trim().is_empty()
                || e.reference.trim().is_empty()
                || e.observation.trim().is_empty()
        })
        || r.scope_impact.trim().is_empty()
        || r.remaining_work.trim().is_empty()
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
fn slice_stale_reasons(
    c: &SliceCandidateContext,
    m: &tect_domain::SlicePlanningSnapshotMaterial,
) -> Vec<String> {
    let mut r = Vec::new();
    if c.snapshot.planning_latest_input != c.candidate_set.latest_input {
        r.push("planning_inputs".into())
    }
    if c.snapshot.method.revision != m.method.revision
        || c.snapshot.method.digest != m.method.digest
    {
        r.push("method".into())
    }
    if c.snapshot.registry_revision != m.registry_revision
        || c.snapshot.registry_digest != m.registry_digest
    {
        r.push("rules".into())
    }
    if c.snapshot.catalogue.revision != m.catalogue.revision
        || c.snapshot.catalogue.digest != m.catalogue.digest
    {
        r.push("pipeline_catalogue".into())
    }
    let mut captured = c.snapshot.result_ids.clone();
    captured.sort_unstable();
    let mut current = c.results.iter().map(|result| result.id).collect::<Vec<_>>();
    current.sort_unstable();
    if captured != current {
        r.push("slice_results".into())
    }
    r
}
