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
    let boundary = invocation.refusal_boundary();
    let result = match invocation {
        PipelineInvocation::Context(query) => {
            let refresh = query.refresh;
            service
                .pipeline_context(context, &query)
                .await
                .and_then(|value| crate::pipeline_output::context(value, capacity, refresh))
        }
        PipelineInvocation::Instruction(query) => service
            .pipeline_instruction(context, &query)
            .await
            .and_then(|value| crate::pipeline_output::instruction(value, capacity)),
        PipelineInvocation::Begin(request) => service
            .pipeline_run_begin(
                context,
                &request,
                &StaticPipelineDefinitions,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::begin(value, capacity)),
        PipelineInvocation::Migrate(request) => service
            .pipeline_run_migrate(context, &request, &StaticPipelineDefinitions)
            .await
            .and_then(|value| {
                serde_json::to_value(value)
                    .map(|value| crate::responses::with_actions(value, Vec::new(), None))
                    .map_err(tect_domain::Error::invalid_arguments_from)
            }),
        PipelineInvocation::Complete(request) => service
            .pipeline_phase_complete(
                context,
                &request,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
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
        PipelineInvocation::ResolveCheckpoint(request) => service
            .pipeline_checkpoint_resolve(
                context,
                &request,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::checkpoint_resolution(value, capacity)),
        PipelineInvocation::EvidenceRegister(request) => service
            .pipeline_evidence_artifact_register(context, &request)
            .await
            .and_then(|value| {
                serde_json::to_value(value).map_err(tect_domain::Error::invalid_arguments_from)
            }),
        PipelineInvocation::EvidenceFinalize(request) => service
            .pipeline_evidence_artifact_finalize(context, &request)
            .await
            .and_then(|value| {
                serde_json::to_value(value).map_err(tect_domain::Error::invalid_arguments_from)
            }),
        PipelineInvocation::EvidenceRead(request) => service
            .pipeline_evidence_artifact_read(context, &request)
            .await
            .and_then(|value| {
                serde_json::to_value(value).map_err(tect_domain::Error::invalid_arguments_from)
            }),
    };
    result.map_err(|error| {
        error.normalize_pipeline_refusal(
            boundary.rule,
            boundary.path,
            boundary.expected,
            boundary.next_action,
            boundary.required,
        )
    })
}
