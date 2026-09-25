use super::*;

impl WorkspaceService {
    pub async fn scope_open(
        &self,
        context: &tect_domain::RequestContext,
        request: &OpenScope,
        source_guidance: &dyn CandidateGuidance,
        slice_guidance: &dyn NativePlanningGuidance,
        guard: &dyn NativePlanningOutputGuard,
    ) -> Result<OpenScopeOutcome> {
        validate_scope_open(request)?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        request.task_context.validate()?;
        if let Some(mut replay) = tx.scope_open_replay(workspace.id, request).await? {
            let opened = open_scope_context_mut(&mut replay);
            tx.slice_candidate_context(workspace.id, opened.scope.id)
                .await?
                .ok_or(Error::NotFound)?;
            let source = tx
                .candidate_context(workspace.id, opened.scope.source_candidate_set_id)
                .await?
                .ok_or(Error::NotFound)?;
            let principal = tx.session_principal(session.id).await?;
            let manifest = tx
                .capture_planning_knowledge(
                    workspace.id,
                    principal,
                    tect_domain::PlanningStage::SliceCandidates,
                    opened.scope.id,
                    opened.scope.revision,
                    opened.planning.candidate_set.latest_input,
                    request.request_id,
                    Some(source.context.candidate_set.program_id),
                    Some(opened.scope.id),
                    Some(&request.task_context),
                    &tect_domain::PlanningMethodSnapshot::from_candidate(
                        &opened.planning.snapshot.method,
                    ),
                )
                .await?;
            opened.planning.planning_knowledge = Some(
                tx.planning_manifest_status(workspace.id, principal, manifest)
                    .await?,
            );
            guard.check_open_scope(&replay)?;
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
        let material = slice_guidance.snapshot(&basis, &[], &[])?;
        let planning_method = tect_domain::PlanningMethodSnapshot::from_candidate(&material.method);
        validate_material(&material)?;
        let mut outcome = tx
            .open_scope(workspace.id, session.id, request, &material)
            .await?;
        let opened = open_scope_context_mut(&mut outcome);
        if let Some(manifest) = consumed {
            tx.register_planning_consumption(
                workspace.id,
                manifest.id,
                "native_scopes",
                opened.scope.id,
                opened.scope.revision,
            )
            .await?;
        }
        let source = tx
            .candidate_context(workspace.id, request.candidate_set_id)
            .await?
            .ok_or(Error::NotFound)?;
        let manifest = tx
            .capture_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::SliceCandidates,
                opened.scope.id,
                opened.scope.revision,
                opened.planning.candidate_set.latest_input,
                request.request_id,
                Some(source.context.candidate_set.program_id),
                Some(opened.scope.id),
                Some(&request.task_context),
                &planning_method,
            )
            .await?;
        opened.planning.planning_knowledge = Some(
            tx.planning_manifest_status(workspace.id, principal, manifest)
                .await?,
        );
        guard.check_open_scope(&outcome)?;
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
            || request.disposition_id.is_some_and(|id| id.is_nil())
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        tx.slice_candidate_context(workspace.id, request.scope_id)
            .await?
            .ok_or(Error::NotFound)?;
        if request.disposition_id.is_some() {
            if let Some(manifest) = tx.slice_open_manifest(workspace.id, request).await? {
                self.validate_pipeline_recommendation_definitions(&manifest)?;
            }
        }
        let value = tx.open_slice(workspace.id, session.id, request).await?;
        let opened = match &value {
            OpenSliceOutcome::Created(slice) | OpenSliceOutcome::Replay(slice) => slice,
        };
        opened.validate_verification_plan_binding()?;
        if request.disposition_id.is_some() && opened.selected_option_id.is_none() {
            return Err(Error::InputConflict);
        }
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
        tx.slice_candidate_context(workspace.id, request.scope_id)
            .await?
            .ok_or(Error::NotFound)?;
        let value = tx
            .record_slice_result(workspace.id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }
}
