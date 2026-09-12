use crate::{
    CandidateGuidance, CandidateOutputGuard, TransactionMode, UnitOfWork, WorkspaceService,
};
use tect_domain::{
    BeginCandidateSet, BeginCandidateSetOutcome, CandidateContext, CandidateContextPage,
    CandidateContextView, Error, RecordCandidateInput, RefreshCandidateSet, Result,
    ReviewCandidateSet, SaveCandidateDraft, StoredCandidateContext, validate_program_input,
};

impl WorkspaceService {
    pub async fn candidate_fragment(
        &self,
        context: &tect_domain::RequestContext,
        candidate_set_id: uuid::Uuid,
        source_ref_id: uuid::Uuid,
        cursor: usize,
        max_bytes: usize,
    ) -> Result<tect_domain::CandidateTextFragment> {
        if candidate_set_id.is_nil() || source_ref_id.is_nil() || max_bytes < 4 {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .candidate_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let fragment = tx
            .candidate_fragment(
                workspace.id,
                candidate_set_id,
                source_ref_id,
                cursor,
                max_bytes,
            )
            .await?;
        tx.commit().await?;
        Ok(fragment)
    }

    async fn candidate_transaction(
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

    pub async fn begin_candidate_set(
        &self,
        context: &tect_domain::RequestContext,
        request: &BeginCandidateSet,
        guidance: &dyn CandidateGuidance,
        guard: &dyn CandidateOutputGuard,
    ) -> Result<BeginCandidateSetOutcome> {
        let (mut tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadWrite)
            .await?;
        if let Some(outcome) = tx.candidate_begin_replay(workspace.id, request).await? {
            guard.check_begin(&outcome)?;
            tx.commit().await?;
            return Ok(outcome);
        }
        validate_program_input(request.request_id, &request.input)?;
        if request.program_id.is_nil() || request.program_revision < 1 {
            return Err(Error::InvalidArguments);
        }
        let program = tx
            .program(workspace.id, request.program_id, false)
            .await?
            .ok_or(Error::NotFound)?;
        if program.revision != request.program_revision {
            return Err(Error::StaleRevision);
        }
        let selected = tx
            .selected_worktrees(workspace.id, session.host_id, session.id)
            .await?;
        let material = guidance.snapshot(program, selected)?;
        guard.check_material(&material)?;
        let outcome = tx
            .ensure_candidate_set(
                workspace.id,
                session.id,
                request,
                guard.input_bytes(&request.input)?,
                &material,
            )
            .await?;
        guard.check_begin(&outcome)?;
        tx.commit().await?;
        Ok(outcome)
    }

    pub async fn candidate_context(
        &self,
        context: &tect_domain::RequestContext,
        candidate_set_id: uuid::Uuid,
        view: CandidateContextView,
        after: Option<i64>,
        limit: u32,
        guidance: &dyn CandidateGuidance,
    ) -> Result<CandidateContextPage> {
        crate::scope_candidate_pages::validate_page(candidate_set_id, after, limit)?;
        let (mut tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let mut stored = tx
            .candidate_context(workspace.id, candidate_set_id)
            .await?
            .ok_or(Error::NotFound)?;
        let current_program = tx
            .program(workspace.id, stored.context.candidate_set.program_id, false)
            .await?
            .ok_or(Error::NotFound)?;
        let selected = tx
            .selected_worktrees(workspace.id, session.host_id, session.id)
            .await?;
        let current = guidance.snapshot(current_program, selected)?;
        stored.context.current_program_revision = current.program.revision;
        stored.context.stale_reasons = stale_reasons(&stored.context, &current);
        let page = crate::scope_candidate_pages::context_page(
            &mut *tx,
            workspace.id,
            stored,
            view,
            after,
            limit,
        )
        .await?;
        tx.commit().await?;
        Ok(page)
    }

    pub async fn save_candidate_draft(
        &self,
        context: &tect_domain::RequestContext,
        request: &SaveCandidateDraft,
        guidance: &dyn CandidateGuidance,
        guard: &dyn CandidateOutputGuard,
    ) -> Result<StoredCandidateContext> {
        let (mut tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::CandidateReceiptRequest::SaveDraft(request.clone());
        if let Some(stored) = tx.candidate_receipt(workspace.id, &receipt).await? {
            guard.check_stored(&stored)?;
            tx.commit().await?;
            return Ok(stored);
        }
        request.draft.validate()?;
        validate_write(
            request.candidate_set_id,
            request.snapshot_id,
            request.revision,
            request.input_cursor,
            request.request_id,
        )?;
        ensure_fresh(
            &mut *tx,
            workspace.id,
            session.host_id,
            session.id,
            request.candidate_set_id,
            guidance,
        )
        .await?;
        let stored = tx.save_candidate_draft(workspace.id, request).await?;
        guard.check_draft(stored.draft.as_ref().ok_or(Error::InternalInvariant)?)?;
        guard.check_stored(&stored)?;
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn review_candidate_set(
        &self,
        context: &tect_domain::RequestContext,
        request: &ReviewCandidateSet,
        guidance: &dyn CandidateGuidance,
        guard: &dyn CandidateOutputGuard,
    ) -> Result<StoredCandidateContext> {
        let (mut tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::CandidateReceiptRequest::Review(request.clone());
        if let Some(stored) = tx.candidate_receipt(workspace.id, &receipt).await? {
            guard.check_stored(&stored)?;
            tx.commit().await?;
            return Ok(stored);
        }
        validate_write(
            request.candidate_set_id,
            request.snapshot_id,
            request.revision,
            request.input_cursor,
            request.request_id,
        )?;
        ensure_fresh(
            &mut *tx,
            workspace.id,
            session.host_id,
            session.id,
            request.candidate_set_id,
            guidance,
        )
        .await?;
        let stored = tx.save_candidate_review(workspace.id, request).await?;
        guard.check_stored(&stored)?;
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn record_candidate_input(
        &self,
        context: &tect_domain::RequestContext,
        request: &RecordCandidateInput,
        guard: &dyn CandidateOutputGuard,
    ) -> Result<StoredCandidateContext> {
        let (mut tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::CandidateReceiptRequest::RecordInput(request.clone());
        if let Some(mut stored) = tx.candidate_receipt(workspace.id, &receipt).await? {
            stored.context.stale_reasons = vec!["planning_inputs".into()];
            guard.check_stored(&stored)?;
            tx.commit().await?;
            return Ok(stored);
        }
        validate_program_input(request.request_id, &request.input)?;
        if request.candidate_set_id.is_nil() || request.revision < 1 {
            return Err(Error::InvalidArguments);
        }
        let mut stored = tx
            .record_candidate_input(
                workspace.id,
                session.id,
                request,
                guard.input_bytes(&request.input)?,
            )
            .await?;
        stored.context.stale_reasons = vec!["planning_inputs".into()];
        guard.check_stored(&stored)?;
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn refresh_candidate_set(
        &self,
        context: &tect_domain::RequestContext,
        request: &RefreshCandidateSet,
        guidance: &dyn CandidateGuidance,
        guard: &dyn CandidateOutputGuard,
    ) -> Result<StoredCandidateContext> {
        let (mut tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::CandidateReceiptRequest::Refresh(request.clone());
        if let Some(stored) = tx.candidate_receipt(workspace.id, &receipt).await? {
            guard.check_stored(&stored)?;
            tx.commit().await?;
            return Ok(stored);
        }
        if request.candidate_set_id.is_nil()
            || request.request_id.is_nil()
            || request.revision < 1
            || request.program_revision < 1
        {
            return Err(Error::InvalidArguments);
        }
        let stored = tx
            .candidate_context(workspace.id, request.candidate_set_id)
            .await?
            .ok_or(Error::NotFound)?;
        let program = tx
            .program(workspace.id, stored.context.candidate_set.program_id, false)
            .await?
            .ok_or(Error::NotFound)?;
        if program.revision != request.program_revision {
            return Err(Error::StaleRevision);
        }
        let selected = tx
            .selected_worktrees(workspace.id, session.host_id, session.id)
            .await?;
        let material = guidance.snapshot(program, selected)?;
        guard.check_material(&material)?;
        let stored = tx
            .refresh_candidate_set(workspace.id, request, &material)
            .await?;
        guard.check_stored(&stored)?;
        tx.commit().await?;
        Ok(stored)
    }
}

async fn ensure_fresh(
    tx: &mut dyn UnitOfWork,
    workspace_id: uuid::Uuid,
    host_id: uuid::Uuid,
    session_id: uuid::Uuid,
    candidate_set_id: uuid::Uuid,
    guidance: &dyn CandidateGuidance,
) -> Result<()> {
    let stored = tx
        .candidate_context(workspace_id, candidate_set_id)
        .await?
        .ok_or(Error::NotFound)?;
    let program = tx
        .program(workspace_id, stored.context.candidate_set.program_id, false)
        .await?
        .ok_or(Error::NotFound)?;
    let selected = tx
        .selected_worktrees(workspace_id, host_id, session_id)
        .await?;
    let current = guidance.snapshot(program, selected)?;
    if stale_reasons(&stored.context, &current).is_empty() {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

fn stale_reasons(
    context: &CandidateContext,
    material: &tect_domain::CandidateSnapshotMaterial,
) -> Vec<String> {
    let snapshot = &context.snapshot;
    let mut reasons = Vec::new();
    if snapshot.program_revision != material.program.revision
        || snapshot.program_latest_input != material.program.latest_input
    {
        reasons.push("program".into());
    }
    if snapshot.planning_latest_input != context.candidate_set.latest_input {
        reasons.push("planning_inputs".into());
    }
    if snapshot.selected_sources_digest != material.selected_sources_digest {
        reasons.push("selected_sources".into());
    }
    if snapshot.method.revision != material.method.revision
        || snapshot.method.digest != material.method.digest
    {
        reasons.push("method".into());
    }
    if snapshot.registry_revision != material.registry_revision
        || snapshot.registry_digest != material.registry_digest
    {
        reasons.push("rules".into());
    }
    reasons
}

fn validate_write(
    id: uuid::Uuid,
    snapshot: uuid::Uuid,
    revision: i64,
    cursor: i64,
    request: uuid::Uuid,
) -> Result<()> {
    if id.is_nil() || snapshot.is_nil() || request.is_nil() || revision < 1 || cursor < 0 {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
