use super::*;

pub(super) async fn execute_program(
    context: &RequestContext,
    invocation: ProgramInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let guard = ProgramEncoding { capacity };
    let guidance = program_output::StaticProgramGuidance;
    match invocation {
        ProgramInvocation::Begin {
            request_id,
            input,
            task_context,
        } => service
            .begin_program(
                context,
                request_id,
                &input,
                &task_context,
                &guidance,
                &guard,
            )
            .await
            .and_then(program_output::begun),
        ProgramInvocation::Get {
            program_id,
            after_input,
            limit,
            window,
            program_revision,
        } => service
            .get_program(context, program_id, after_input, limit, &guidance)
            .await
            .and_then(|page| {
                program_output::page_read(
                    page,
                    after_input,
                    limit,
                    &window,
                    program_revision,
                    capacity,
                )
            }),
        ProgramInvocation::Save(changes) => service
            .save_program(context, &changes, &guidance, &guard)
            .await
            .and_then(program_output::saved),
        ProgramInvocation::Record {
            program_id,
            request_id,
            input,
            task_context,
        } => service
            .record_program_input(
                context,
                program_id,
                request_id,
                &input,
                task_context.as_ref(),
                &guidance,
                &guard,
            )
            .await
            .and_then(program_output::program),
        ProgramInvocation::Refresh(request) => service
            .refresh_program_knowledge(context, &request, &guidance, &guard)
            .await
            .and_then(program_output::program),
        ProgramInvocation::List {
            after,
            after_selector,
            limit,
            window,
            workspace_id,
        } => {
            let (bound_workspace_id, list) =
                service.list_programs_bound(context, after, limit).await?;
            if workspace_id.is_some_and(|expected| expected != bound_workspace_id) {
                return Err(Error::InvalidArguments);
            }
            program_output::list_read::read(
                list,
                bound_workspace_id,
                after_selector.as_deref(),
                limit,
                &window,
                capacity,
            )
        }
        ProgramInvocation::ReadSkill => {
            service.read_program_skill(context).await?;
            Ok(program_output::skill())
        }
    }
}
