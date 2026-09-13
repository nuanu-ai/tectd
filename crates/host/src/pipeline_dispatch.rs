use crate::pipeline_definitions::StaticPipelineDefinitions;
use crate::pipeline_tools::PipelineInvocation;
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, Result};

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: PipelineInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    match invocation {
        PipelineInvocation::Context(query) => service
            .pipeline_context(context, &query)
            .await
            .and_then(|value| crate::pipeline_output::context(value, capacity)),
        PipelineInvocation::Begin(request) => service
            .pipeline_run_begin(context, &request, &StaticPipelineDefinitions)
            .await
            .and_then(|value| crate::pipeline_output::begin(value, capacity)),
        PipelineInvocation::Complete(request) => service
            .pipeline_phase_complete(context, &request)
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
        PipelineInvocation::Input(request) => service
            .pipeline_input_record(context, &request)
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
        PipelineInvocation::EscalateDelivery(request) => service
            .pipeline_delivery_escalate(context, &request)
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
    }
}
