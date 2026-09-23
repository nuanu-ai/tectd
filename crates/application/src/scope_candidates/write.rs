use super::*;

impl WorkspaceService {
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
        let principal = tx.session_principal(session.id).await?;
        let receipt = tect_domain::CandidateReceiptRequest::SaveDraft(request.clone());
        let replay = if request.selected_advisory.is_some() {
            tx.selected_candidate_receipt(workspace.id, principal, session.id, request)
                .await?
        } else {
            tx.candidate_receipt(workspace.id, &receipt).await?
        };
        if let Some(mut stored) = replay {
            stored.context.planning_knowledge = tx
                .planning_consumption_status(
                    workspace.id,
                    principal,
                    "scope_candidate_drafts",
                    request.candidate_set_id,
                    stored.context.candidate_set.revision,
                )
                .await?;
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
        let consumed = tx
            .require_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Scope,
                request.candidate_set_id,
                request.consumed_knowledge.as_ref(),
            )
            .await?;
        let mut stored = if request.selected_advisory.is_some() {
            tx.save_selected_candidate_draft(workspace.id, principal, session.id, request)
                .await?
        } else {
            tx.save_candidate_draft(workspace.id, request).await?
        };
        if let Some(manifest) = &consumed {
            tx.register_planning_consumption(
                workspace.id,
                manifest.id,
                "scope_candidate_drafts",
                request.candidate_set_id,
                stored.context.candidate_set.revision,
            )
            .await?;
            tx.register_planning_receipt_copy(
                workspace.id,
                manifest.id,
                "scope_candidate_receipts",
                request.candidate_set_id,
                stored.context.candidate_set.revision,
                "save_draft",
                request.request_id,
            )
            .await?;
        }
        stored.context.planning_knowledge = Some(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Scope,
                request.candidate_set_id,
            )
            .await?,
        );
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
        if let Some(mut stored) = tx.candidate_receipt(workspace.id, &receipt).await? {
            let principal = tx.session_principal(session.id).await?;
            stored.context.planning_knowledge = tx
                .planning_consumption_status(
                    workspace.id,
                    principal,
                    "scope_candidate_reviews",
                    request.candidate_set_id,
                    stored.context.candidate_set.revision,
                )
                .await?;
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
        let principal = tx.session_principal(session.id).await?;
        let consumed = tx
            .require_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Scope,
                request.candidate_set_id,
                request.consumed_knowledge.as_ref(),
            )
            .await?;
        let mut stored = tx.save_candidate_review(workspace.id, request).await?;
        if let Some(manifest) = &consumed {
            tx.register_planning_consumption(
                workspace.id,
                manifest.id,
                "scope_candidate_reviews",
                request.candidate_set_id,
                stored.context.candidate_set.revision,
            )
            .await?;
            tx.register_planning_receipt_copy(
                workspace.id,
                manifest.id,
                "scope_candidate_receipts",
                request.candidate_set_id,
                stored.context.candidate_set.revision,
                "save_review",
                request.request_id,
            )
            .await?;
        }
        stored.context.planning_knowledge = Some(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Scope,
                request.candidate_set_id,
            )
            .await?,
        );
        guard.check_stored(&stored)?;
        tx.commit().await?;
        Ok(stored)
    }
}
