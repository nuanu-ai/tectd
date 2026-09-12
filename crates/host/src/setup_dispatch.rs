use crate::setup_output::{self, SetupEncoding};
use crate::setup_tools::SetupInvocation;
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, Result};

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: SetupInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let guard = SetupEncoding { capacity };
    match invocation {
        SetupInvocation::Inspect { task_directory } => service
            .inspect_setup(context, task_directory.as_deref(), capacity)
            .await
            .and_then(|discovery| crate::workspace_output::discovery(discovery, capacity)),
        SetupInvocation::Begin { request_id, input } => service
            .begin_setup(context, request_id, &input, &guard)
            .await
            .map(setup_output::saved),
        SetupInvocation::Get {
            setup_id,
            after_input,
            limit,
        } => service
            .get_setup(context, setup_id, after_input, limit, capacity)
            .await
            .and_then(|page| setup_output::page(page, capacity)),
        SetupInvocation::Save(changes) => service
            .save_setup(context, &changes, &guard)
            .await
            .map(setup_output::saved),
        SetupInvocation::Record {
            setup_id,
            revision,
            request_id,
            input,
        } => service
            .record_setup_input(context, setup_id, revision, request_id, &input, &guard)
            .await
            .map(setup_output::saved),
        SetupInvocation::Apply { setup_id, revision } => service
            .apply_setup(context, setup_id, revision, &guard)
            .await
            .map(setup_output::applied),
        SetupInvocation::ReadSkill => {
            service.read_program_skill(context).await?;
            Ok(setup_output::skill())
        }
    }
}
