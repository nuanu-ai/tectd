use crate::{ProgramGuidance, ProgramOutputGuard, TransactionMode, UnitOfWork, WorkspaceService};
use tect_domain::{
    Error, NewProgramInput, Program, ProgramCursor, ProgramList, ProgramPage, ProgramSummary,
    RequestContext, Result, SaveProgram, Session, Workspace, validate_program_input,
};
use uuid::Uuid;

fn with_current_program_method(
    mut status: tect_domain::PlanningKnowledgeStatus,
    method: &tect_domain::PlanningMethodSnapshot,
) -> tect_domain::PlanningKnowledgeStatus {
    if status
        .manifest
        .as_ref()
        .is_some_and(|manifest| manifest.needs.method != *method)
    {
        status.stale_reasons.push("planning_method".into());
        status.stale_reasons.sort();
        status.stale_reasons.dedup();
    }
    status
}

fn require_current_program_method(
    manifest: Option<tect_domain::PlanningKnowledgeManifest>,
    method: &tect_domain::PlanningMethodSnapshot,
) -> Result<Option<tect_domain::PlanningKnowledgeManifest>> {
    if manifest
        .as_ref()
        .is_some_and(|manifest| manifest.needs.method != *method)
    {
        Err(Error::StaleContext)
    } else {
        Ok(manifest)
    }
}

impl WorkspaceService {
    async fn program_transaction(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<(Box<dyn UnitOfWork>, Workspace, Session)> {
        let (mut tx, identity) = self.authorized(context, mode).await?;
        if mode == TransactionMode::ReadWrite {
            tx.lock_native_session(identity.host_id, &context.native_session_id)
                .await?;
        }
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        Ok((tx, workspace, session))
    }

    pub async fn begin_program(
        &self,
        context: &RequestContext,
        request_id: Uuid,
        input: &str,
        task_context: &tect_domain::PlanningTaskContext,
        guidance: &dyn ProgramGuidance,
        guard: &dyn ProgramOutputGuard,
    ) -> Result<Program> {
        let (mut tx, workspace, session) = self
            .program_transaction(context, TransactionMode::ReadWrite)
            .await?;
        validate_program_input(request_id, input)?;
        let input = NewProgramInput {
            request_id,
            input: input.to_owned(),
            encoded_bytes: guard.input_bytes(input)?,
        };
        let mut program = tx.ensure_program(workspace.id, session.id, &input).await?;
        let principal = tx.session_principal(session.id).await?;
        let method = guidance.planning_method();
        let manifest = tx
            .capture_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Program,
                program.id,
                program.revision,
                1,
                request_id,
                Some(program.id),
                None,
                Some(task_context),
                &method,
            )
            .await?;
        program.planning_knowledge = Some(with_current_program_method(
            tx.planning_manifest_status(workspace.id, principal, manifest)
                .await?,
            &method,
        ));
        guard.check(&program)?;
        tx.commit().await?;
        Ok(program)
    }

    pub async fn get_program(
        &self,
        context: &RequestContext,
        program_id: Uuid,
        after_input: Option<i64>,
        limit: u32,
        guidance: &dyn ProgramGuidance,
    ) -> Result<ProgramPage> {
        let (mut tx, workspace, session) = self
            .program_transaction(context, TransactionMode::ReadOnly)
            .await?;
        validate_page(limit)?;
        if program_id.is_nil() || after_input.is_some_and(|cursor| cursor < 0) {
            return Err(Error::InvalidArguments);
        }
        let mut program = tx
            .program(workspace.id, program_id, false)
            .await?
            .ok_or(Error::NotFound)?;
        let principal = tx.session_principal(session.id).await?;
        program.planning_knowledge = Some(with_current_program_method(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Program,
                program.id,
            )
            .await?,
            &guidance.planning_method(),
        ));
        let after = after_input.unwrap_or(program.input_cursor);
        let mut inputs = tx
            .program_inputs(workspace.id, program_id, after, limit + 1)
            .await?;
        let next_after_input = if inputs.len() > limit as usize {
            inputs.pop();
            inputs.last().map(|entry| entry.sequence)
        } else {
            None
        };
        tx.commit().await?;
        Ok(ProgramPage {
            program,
            inputs,
            next_after_input,
        })
    }

    pub async fn save_program(
        &self,
        context: &RequestContext,
        changes: &SaveProgram,
        guidance: &dyn ProgramGuidance,
        guard: &dyn ProgramOutputGuard,
    ) -> Result<Program> {
        let (mut tx, workspace, session) = self
            .program_transaction(context, TransactionMode::ReadWrite)
            .await?;
        changes.validate()?;
        let current = tx
            .program(workspace.id, changes.program_id, true)
            .await?
            .ok_or(Error::NotFound)?;
        let principal = tx.session_principal(session.id).await?;
        let method = guidance.planning_method();
        let current_knowledge = with_current_program_method(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Program,
                current.id,
            )
            .await?,
            &method,
        );
        if current_knowledge
            .stale_reasons
            .iter()
            .any(|reason| reason == "planning_method" || reason == "planning_policy")
        {
            return Err(Error::StaleContext);
        }
        let consumed = require_current_program_method(
            tx.require_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Program,
                current.id,
                changes.consumed_knowledge.as_ref(),
            )
            .await?,
            &method,
        )?;
        let mut program = current.saved(changes)?;
        guard.check(&program)?;
        tx.update_program(&program).await?;
        if let Some(manifest) = consumed {
            tx.register_planning_consumption(
                workspace.id,
                manifest.id,
                "programs",
                program.id,
                program.revision,
            )
            .await?;
        }
        program.planning_knowledge = Some(with_current_program_method(
            tx.planning_knowledge_status(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Program,
                program.id,
            )
            .await?,
            &method,
        ));
        guard.check(&program)?;
        tx.commit().await?;
        Ok(program)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_program_input(
        &self,
        context: &RequestContext,
        program_id: Uuid,
        request_id: Uuid,
        input: &str,
        task_context: Option<&tect_domain::PlanningTaskContext>,
        guidance: &dyn ProgramGuidance,
        guard: &dyn ProgramOutputGuard,
    ) -> Result<Program> {
        let (mut tx, workspace, session) = self
            .program_transaction(context, TransactionMode::ReadWrite)
            .await?;
        validate_program_input(request_id, input)?;
        if program_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let current = tx
            .program(workspace.id, program_id, true)
            .await?
            .ok_or(Error::NotFound)?;
        if let Some(original) = tx
            .program_input(workspace.id, program_id, request_id)
            .await?
        {
            if original.input != input {
                return Err(Error::InputConflict);
            }
            let principal = tx.session_principal(session.id).await?;
            let method = guidance.planning_method();
            let manifest = tx
                .capture_planning_knowledge(
                    workspace.id,
                    principal,
                    tect_domain::PlanningStage::Program,
                    current.id,
                    current.revision,
                    original.sequence,
                    request_id,
                    Some(current.id),
                    None,
                    task_context,
                    &method,
                )
                .await?;
            let mut current = current;
            current.planning_knowledge = Some(with_current_program_method(
                tx.planning_manifest_status(workspace.id, principal, manifest)
                    .await?,
                &method,
            ));
            guard.check(&current)?;
            tx.commit().await?;
            return Ok(current);
        }
        let input = NewProgramInput {
            request_id,
            input: input.to_owned(),
            encoded_bytes: guard.input_bytes(input)?,
        };
        let mut program = current.with_new_input(input.encoded_bytes)?;
        guard.check(&program)?;
        tx.insert_program_input(
            workspace.id,
            program_id,
            session.id,
            program.latest_input,
            &input,
        )
        .await?;
        tx.update_program(&program).await?;
        let principal = tx.session_principal(session.id).await?;
        let method = guidance.planning_method();
        let manifest = tx
            .capture_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Program,
                program.id,
                program.revision,
                program.latest_input,
                request_id,
                Some(program.id),
                None,
                task_context,
                &method,
            )
            .await?;
        program.planning_knowledge = Some(with_current_program_method(
            tx.planning_manifest_status(workspace.id, principal, manifest)
                .await?,
            &method,
        ));
        guard.check(&program)?;
        tx.commit().await?;
        Ok(program)
    }

    pub async fn refresh_program_knowledge(
        &self,
        context: &RequestContext,
        request: &tect_domain::RefreshProgramKnowledge,
        guidance: &dyn ProgramGuidance,
        guard: &dyn ProgramOutputGuard,
    ) -> Result<Program> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .program_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal = tx.session_principal(session.id).await?;
        if let Some(program) = tx
            .program_knowledge_refresh_replay(workspace.id, principal, request)
            .await?
        {
            let mut program = program;
            let method = guidance.planning_method();
            if let Some(manifest) = program
                .planning_knowledge
                .as_ref()
                .and_then(|status| status.manifest.clone())
            {
                program.planning_knowledge = Some(with_current_program_method(
                    tx.planning_manifest_status(workspace.id, principal, manifest)
                        .await?,
                    &method,
                ));
            }
            guard.check(&program)?;
            tx.commit().await?;
            return Ok(program);
        }
        let current = tx
            .program(workspace.id, request.program_id, true)
            .await?
            .ok_or(Error::NotFound)?;
        let mut program = current.refreshed(request)?;
        tx.update_program(&program).await?;
        let method = guidance.planning_method();
        let manifest = tx
            .capture_planning_knowledge(
                workspace.id,
                principal,
                tect_domain::PlanningStage::Program,
                program.id,
                program.revision,
                program.latest_input,
                request.request_id,
                Some(program.id),
                None,
                request.task_context.as_ref(),
                &method,
            )
            .await?;
        program.planning_knowledge = Some(with_current_program_method(
            tx.planning_manifest_status(workspace.id, principal, manifest)
                .await?,
            &method,
        ));
        guard.check(&program)?;
        tx.save_program_knowledge_refresh_receipt(workspace.id, request, &program)
            .await?;
        tx.commit().await?;
        Ok(program)
    }

    pub async fn list_programs(
        &self,
        context: &RequestContext,
        after: Option<ProgramCursor>,
        limit: u32,
    ) -> Result<ProgramList> {
        let (mut tx, workspace, _) = self
            .program_transaction(context, TransactionMode::ReadOnly)
            .await?;
        validate_page(limit)?;
        let entries = tx.list_programs(workspace.id, after, limit + 1).await?;
        let page = bounded_program_list(entries, limit);
        tx.commit().await?;
        Ok(page)
    }

    /// The body is build-bound in the host, but access still belongs to the application.
    pub async fn read_program_skill(&self, context: &RequestContext) -> Result<()> {
        let (tx, _, _) = self
            .program_transaction(context, TransactionMode::ReadOnly)
            .await?;
        tx.commit().await
    }
}

pub(crate) fn bounded_program_list(mut programs: Vec<ProgramSummary>, limit: u32) -> ProgramList {
    let next_after = if programs.len() > limit as usize {
        programs.pop();
        programs.last().map(|program| program.cursor().encode())
    } else {
        None
    };
    ProgramList {
        programs,
        next_after,
    }
}

fn validate_page(limit: u32) -> Result<()> {
    if !(1..=100).contains(&limit) {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}
