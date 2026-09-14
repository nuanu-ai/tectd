use super::*;

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

    pub async fn slice_candidate_context(
        &self,
        context: &tect_domain::RequestContext,
        query: &SliceCandidateContextQuery,
        guidance: &dyn NativePlanningGuidance,
    ) -> Result<SliceCandidateContext> {
        if query.scope_id.is_nil() || query.limit == 0 || query.limit > 100 {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let mut stored = tx
            .slice_candidate_context(workspace.id, query.scope_id)
            .await?
            .ok_or(Error::NotFound)?;
        let basis = basis_from_context(&stored)?;
        let current = guidance.snapshot(&basis, &stored.inputs, &stored.results)?;
        stored.stale_reasons = slice_stale_reasons(&stored, &current);
        let principal = tx.session_principal(session.id).await?;
        stored.planning_knowledge = Some(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::SliceCandidates,
                query.scope_id,
            )
            .await?,
        );
        tx.commit().await?;
        Ok(stored)
    }

    pub async fn save_slice_candidate_draft(
        &self,
        context: &tect_domain::RequestContext,
        request: &SaveSliceCandidateDraft,
        guidance: &dyn NativePlanningGuidance,
        guard: &dyn NativePlanningOutputGuard,
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
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::NativePlanningReceiptRequest::SaveDraft(request.clone());
        if let Some(mut value) = tx.native_planning_receipt(workspace.id, &receipt).await? {
            let principal = tx.session_principal(session.id).await?;
            value.planning_knowledge = tx
                .planning_consumption_status(
                    workspace.id,
                    principal,
                    "slice_candidate_drafts",
                    request.candidate_set_id,
                    value.candidate_set.revision,
                )
                .await?;
            guard.check_context(&value)?;
            tx.commit().await?;
            return Ok(value);
        }
        ensure_slice_fresh(&mut *tx, workspace.id, request.scope_id, guidance).await?;
        let principal = tx.session_principal(session.id).await?;
        let consumed = tx
            .require_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::SliceCandidates,
                request.scope_id,
                request.consumed_knowledge.as_ref(),
            )
            .await?;
        let mut value = tx.save_slice_candidate_draft(workspace.id, request).await?;
        if let Some(manifest) = &consumed {
            tx.register_planning_consumption(
                workspace.id,
                manifest.id,
                "slice_candidate_drafts",
                request.candidate_set_id,
                value.candidate_set.revision,
            )
            .await?;
            tx.register_planning_receipt_copy(
                workspace.id,
                manifest.id,
                "native_planning_receipts",
                request.candidate_set_id,
                value.candidate_set.revision,
                "save_slice_draft",
                request.request_id,
            )
            .await?;
        }
        value.planning_knowledge = Some(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::SliceCandidates,
                request.scope_id,
            )
            .await?,
        );
        guard.check_context(&value)?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn review_slice_candidate_set(
        &self,
        context: &tect_domain::RequestContext,
        request: &ReviewSliceCandidateSet,
        guidance: &dyn NativePlanningGuidance,
        guard: &dyn NativePlanningOutputGuard,
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
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::NativePlanningReceiptRequest::Review(request.clone());
        if let Some(mut value) = tx.native_planning_receipt(workspace.id, &receipt).await? {
            let principal = tx.session_principal(session.id).await?;
            value.planning_knowledge = tx
                .planning_consumption_status(
                    workspace.id,
                    principal,
                    "slice_candidate_reviews",
                    request.candidate_set_id,
                    value.candidate_set.revision,
                )
                .await?;
            guard.check_context(&value)?;
            tx.commit().await?;
            return Ok(value);
        }
        ensure_slice_fresh(&mut *tx, workspace.id, request.scope_id, guidance).await?;
        let principal = tx.session_principal(session.id).await?;
        let consumed = tx
            .require_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::SliceCandidates,
                request.scope_id,
                request.consumed_knowledge.as_ref(),
            )
            .await?;
        let mut value = tx.review_slice_candidate_set(workspace.id, request).await?;
        if let Some(manifest) = &consumed {
            tx.register_planning_consumption(
                workspace.id,
                manifest.id,
                "slice_candidate_reviews",
                request.candidate_set_id,
                value.candidate_set.revision,
            )
            .await?;
            tx.register_planning_receipt_copy(
                workspace.id,
                manifest.id,
                "native_planning_receipts",
                request.candidate_set_id,
                value.candidate_set.revision,
                "review_slice_set",
                request.request_id,
            )
            .await?;
        }
        value.planning_knowledge = Some(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::SliceCandidates,
                request.scope_id,
            )
            .await?,
        );
        guard.check_context(&value)?;
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
        guard: &dyn NativePlanningOutputGuard,
    ) -> Result<SliceCandidateContext> {
        if request.scope_id.is_nil()
            || request.candidate_set_id.is_nil()
            || request.request_id.is_nil()
            || request.revision < 1
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let receipt = tect_domain::NativePlanningReceiptRequest::Refresh(request.clone());
        if let Some(task_context) = &request.task_context {
            task_context.validate()?;
        }
        if let Some(mut value) = tx.native_planning_receipt(workspace.id, &receipt).await? {
            let principal = tx.session_principal(session.id).await?;
            let source = tx
                .candidate_context(workspace.id, value.scope.source_candidate_set_id)
                .await?
                .ok_or(Error::NotFound)?;
            let manifest = tx
                .capture_planning_knowledge(
                    workspace.id,
                    principal,
                    tect_domain::PlanningStage::SliceCandidates,
                    value.scope.id,
                    value.scope.revision,
                    value.candidate_set.latest_input,
                    request.request_id,
                    Some(source.context.candidate_set.program_id),
                    Some(value.scope.id),
                    request.task_context.as_ref(),
                    &tect_domain::PlanningMethodSnapshot::from_candidate(&value.snapshot.method),
                )
                .await?;
            value.planning_knowledge = Some(
                tx.planning_manifest_status(workspace.id, principal, manifest)
                    .await?,
            );
            guard.check_context(&value)?;
            tx.commit().await?;
            return Ok(value);
        }
        let stored = tx
            .slice_candidate_context(workspace.id, request.scope_id)
            .await?
            .ok_or(Error::NotFound)?;
        let basis = basis_from_context(&stored)?;
        let material = guidance.snapshot(&basis, &stored.inputs, &stored.results)?;
        let planning_method = tect_domain::PlanningMethodSnapshot::from_candidate(&material.method);
        validate_material(&material)?;
        let mut value = tx
            .refresh_slice_candidate_set(workspace.id, request, &material)
            .await?;
        let principal = tx.session_principal(session.id).await?;
        let source = tx
            .candidate_context(workspace.id, value.scope.source_candidate_set_id)
            .await?
            .ok_or(Error::NotFound)?;
        let manifest = tx
            .capture_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::SliceCandidates,
                value.scope.id,
                value.scope.revision,
                value.candidate_set.latest_input,
                request.request_id,
                Some(source.context.candidate_set.program_id),
                Some(value.scope.id),
                request.task_context.as_ref(),
                &planning_method,
            )
            .await?;
        value.planning_knowledge = Some(
            tx.planning_manifest_status(workspace.id, principal, manifest)
                .await?,
        );
        guard.check_context(&value)?;
        tx.commit().await?;
        Ok(value)
    }
}
