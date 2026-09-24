use crate::{
    CandidateGuidance, CandidateOutputGuard, TransactionMode, UnitOfWork, WorkspaceService,
};
use tect_domain::{
    AdvisoryCapability, AdvisoryOpportunity, BeginCandidateSet, BeginCandidateSetOutcome,
    CandidateContext, CandidateContextPage, CandidateContextQuery, Error,
    MatrixDecompositionParent, RecordCandidateInput, RefreshCandidateSet, Result,
    ReviewCandidateSet, SaveCandidateDraft, StoredCandidateContext, validate_program_input,
};

#[cfg(test)]
mod parent_tests;
mod write;

impl WorkspaceService {
    pub async fn candidate_fragment(
        &self,
        context: &tect_domain::RequestContext,
        candidate_set_id: uuid::Uuid,
        draft_revision: Option<i64>,
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
        let snapshot_id = match draft_revision {
            Some(revision) if revision >= 2 => Some(
                tx.historical_candidate_draft(workspace.id, candidate_set_id, revision)
                    .await?
                    .ok_or(Error::NotFound)?
                    .snapshot
                    .id,
            ),
            Some(_) => return Err(Error::InvalidArguments),
            None => None,
        };
        let fragment = tx
            .candidate_fragment(
                workspace.id,
                candidate_set_id,
                snapshot_id,
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
        // The registered advisory decision is committed before candidate work.
        // A later validation/guard/store failure must not erase its audit fact.
        let (mut opportunity_tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadWrite)
            .await?;
        if request.program_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        if let Some(parent) = &request.parent_matrix {
            parent.validate()?;
            // An exact committed retry survives later changes to the Matrix
            // task. The stored origin payload compares parent and rationale.
            if let Some(BeginCandidateSetOutcome::Replay(outcome)) = opportunity_tx
                .candidate_begin_replay(workspace.id, request)
                .await?
            {
                let replay = BeginCandidateSetOutcome::Replay(outcome);
                guard.check_begin(&replay)?;
                opportunity_tx.commit().await?;
                return Ok(replay);
            }
            let task = opportunity_tx
                .lock_matrix_task(workspace.id, parent.task_id)
                .await?
                .ok_or(Error::NotFound)?;
            let opportunity = opportunity_tx
                .matrix_decomposition_parent(workspace.id, parent.opportunity_id)
                .await?
                .ok_or(Error::NotFound)?;
            validate_matrix_decomposition_parent(parent, &task, &opportunity, workspace.id)?;
        }
        let program = opportunity_tx
            .program(workspace.id, request.program_id, false)
            .await?
            .ok_or(Error::NotFound)?;
        let deterministic_result = request
            .task_context
            .validate()
            .and_then(|_| validate_program_input(request.request_id, &request.input))
            .and({
                if request.program_revision < 1 {
                    Err(Error::InvalidArguments)
                } else if program.revision != request.program_revision {
                    Err(Error::StaleRevision)
                } else {
                    Ok(())
                }
            });
        let principal = opportunity_tx.session_principal(session.id).await?;
        let config = opportunity_tx
            .materialize_advisory_config(workspace.id, principal, session.id)
            .await?;
        let opportunity = crate::advisory::scope_decomposition_opportunity(
            session.id,
            principal,
            request,
            &config,
            tect_domain::AdvisoryRequestPreference::UseWorkspace,
            deterministic_result.is_ok(),
        );
        crate::advisory::commit_scope_opportunity(opportunity_tx, workspace.id, &opportunity)
            .await?;
        deterministic_result?;

        let (mut tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadWrite)
            .await?;
        request.task_context.validate()?;
        if let Some(mut outcome) = tx.candidate_begin_replay(workspace.id, request).await? {
            let candidate = candidate_outcome_context_mut(&mut outcome);
            let principal = tx.session_principal(session.id).await?;
            let manifest = tx
                .capture_planning_knowledge(
                    workspace.id,
                    principal,
                    tect_domain::PlanningStage::Scope,
                    candidate.candidate_set.id,
                    candidate.candidate_set.revision,
                    candidate.candidate_set.latest_input,
                    request.request_id,
                    Some(candidate.candidate_set.program_id),
                    None,
                    Some(&request.task_context),
                    &tect_domain::PlanningMethodSnapshot::from_candidate(
                        &candidate.snapshot.method,
                    ),
                )
                .await?;
            candidate.planning_knowledge = Some(
                tx.planning_manifest_status(workspace.id, principal, manifest)
                    .await?,
            );
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
        let planning_method = tect_domain::PlanningMethodSnapshot::from_candidate(&material.method);
        let mut outcome = tx
            .ensure_candidate_set(
                workspace.id,
                session.id,
                request,
                guard.input_bytes(&request.input)?,
                &material,
            )
            .await?;
        let candidate = candidate_outcome_context_mut(&mut outcome);
        let principal = tx.session_principal(session.id).await?;
        let manifest = tx
            .capture_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Scope,
                candidate.candidate_set.id,
                candidate.candidate_set.revision,
                candidate.candidate_set.latest_input,
                request.request_id,
                Some(candidate.candidate_set.program_id),
                None,
                Some(&request.task_context),
                &planning_method,
            )
            .await?;
        candidate.planning_knowledge = Some(
            tx.planning_manifest_status(workspace.id, principal, manifest)
                .await?,
        );
        guard.check_begin(&outcome)?;
        tx.commit().await?;
        Ok(outcome)
    }

    pub async fn candidate_context(
        &self,
        context: &tect_domain::RequestContext,
        query: &CandidateContextQuery,
        guidance: &dyn CandidateGuidance,
    ) -> Result<CandidateContextPage> {
        crate::scope_candidate_pages::validate_page(
            query.candidate_set_id,
            query.after,
            query.limit,
        )?;
        let (mut tx, workspace, session) = self
            .candidate_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let mut stored = tx
            .candidate_context(workspace.id, query.candidate_set_id)
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
        let principal = tx.session_principal(session.id).await?;
        stored.context.planning_knowledge = Some(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Scope,
                stored.context.candidate_set.id,
            )
            .await?,
        );
        let page = crate::scope_candidate_pages::context_page(
            &mut *tx,
            workspace.id,
            stored,
            query.view,
            query.draft_revision,
            query.after,
            query.limit,
        )
        .await?;
        tx.commit().await?;
        Ok(page)
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
        if let Some(task_context) = &request.task_context {
            task_context.validate()?;
        }
        if let Some(mut stored) = tx.candidate_receipt(workspace.id, &receipt).await? {
            let principal = tx.session_principal(session.id).await?;
            let manifest = tx
                .capture_planning_knowledge(
                    workspace.id,
                    principal,
                    tect_domain::PlanningStage::Scope,
                    stored.context.candidate_set.id,
                    stored.context.candidate_set.revision,
                    stored.context.candidate_set.latest_input,
                    request.request_id,
                    Some(stored.context.candidate_set.program_id),
                    None,
                    request.task_context.as_ref(),
                    &tect_domain::PlanningMethodSnapshot::from_candidate(
                        &stored.context.snapshot.method,
                    ),
                )
                .await?;
            stored.context.planning_knowledge = Some(
                tx.planning_manifest_status(workspace.id, principal, manifest)
                    .await?,
            );
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
        let program_id = stored.context.candidate_set.program_id;
        let material = guidance.snapshot(program, selected)?;
        let planning_method = tect_domain::PlanningMethodSnapshot::from_candidate(&material.method);
        guard.check_material(&material)?;
        let mut stored = tx
            .refresh_candidate_set(workspace.id, request, &material)
            .await?;
        let principal = tx.session_principal(session.id).await?;
        let manifest = tx
            .capture_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Scope,
                stored.context.candidate_set.id,
                stored.context.candidate_set.revision,
                stored.context.candidate_set.latest_input,
                request.request_id,
                Some(program_id),
                None,
                request.task_context.as_ref(),
                &planning_method,
            )
            .await?;
        stored.context.planning_knowledge = Some(
            tx.planning_manifest_status(workspace.id, principal, manifest)
                .await?,
        );
        guard.check_stored(&stored)?;
        tx.commit().await?;
        Ok(stored)
    }
}

fn validate_matrix_decomposition_parent(
    parent: &MatrixDecompositionParent,
    task: &crate::MatrixTaskRevision,
    opportunity: &AdvisoryOpportunity,
    workspace_id: uuid::Uuid,
) -> Result<()> {
    if task.task_id != parent.task_id
        || task.revision != parent.task_revision
        || task.input_digest != parent.input_digest
        || task.choice_set_digest != parent.choice_set_digest
    {
        return Err(Error::StaleRevision);
    }
    if opportunity.id != parent.opportunity_id
        || opportunity.workspace_id != workspace_id
        || opportunity.capability != AdvisoryCapability::EngineeringProfile
        || opportunity.target_kind != "matrix_task"
        || opportunity.target_id != Some(parent.task_id)
        || opportunity.work_revision != Some(parent.task_revision)
        || opportunity.matrix_task_revision != Some(parent.task_revision)
        || opportunity.matrix_choice_set_digest != parent.choice_set_digest
        || opportunity.matrix_verification_digest != parent.verification_digest
        || opportunity.material_digest != parent.opportunity_material_digest
    {
        return Err(Error::InputConflict);
    }
    Ok(())
}

fn candidate_outcome_context_mut(outcome: &mut BeginCandidateSetOutcome) -> &mut CandidateContext {
    match outcome {
        BeginCandidateSetOutcome::Created(value)
        | BeginCandidateSetOutcome::Replay(value)
        | BeginCandidateSetOutcome::Existing(value) => value,
    }
}

pub(crate) async fn ensure_fresh(
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
