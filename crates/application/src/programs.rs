use crate::{ProgramOutputGuard, TransactionMode, UnitOfWork, WorkspaceService};
use tect_domain::{
    Error, NewProgramInput, Program, ProgramCursor, ProgramList, ProgramPage, ProgramSummary,
    RequestContext, Result, SaveProgram, Session, Workspace, validate_program_input,
};
use uuid::Uuid;

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
        let program = tx.ensure_program(workspace.id, session.id, &input).await?;
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
    ) -> Result<ProgramPage> {
        let (mut tx, workspace, _) = self
            .program_transaction(context, TransactionMode::ReadOnly)
            .await?;
        validate_page(limit)?;
        if program_id.is_nil() || after_input.is_some_and(|cursor| cursor < 0) {
            return Err(Error::InvalidArguments);
        }
        let program = tx
            .program(workspace.id, program_id, false)
            .await?
            .ok_or(Error::NotFound)?;
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
        guard: &dyn ProgramOutputGuard,
    ) -> Result<Program> {
        let (mut tx, workspace, _) = self
            .program_transaction(context, TransactionMode::ReadWrite)
            .await?;
        changes.validate()?;
        let current = tx
            .program(workspace.id, changes.program_id, true)
            .await?
            .ok_or(Error::NotFound)?;
        let program = current.saved(changes)?;
        guard.check(&program)?;
        tx.update_program(&program).await?;
        tx.commit().await?;
        Ok(program)
    }

    pub async fn record_program_input(
        &self,
        context: &RequestContext,
        program_id: Uuid,
        request_id: Uuid,
        input: &str,
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
            guard.check(&current)?;
            tx.commit().await?;
            return Ok(current);
        }
        let input = NewProgramInput {
            request_id,
            input: input.to_owned(),
            encoded_bytes: guard.input_bytes(input)?,
        };
        let program = current.with_new_input(input.encoded_bytes)?;
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
