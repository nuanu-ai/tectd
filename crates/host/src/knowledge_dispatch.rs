use crate::knowledge_tools::KnowledgeInvocation;
use crate::pipeline_definitions::StaticPipelineDefinitions;
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, Result};

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: KnowledgeInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    match invocation {
        KnowledgeInvocation::Context(query) => service
            .knowledge_context(context, &query)
            .await
            .and_then(|value| crate::knowledge_output::context(value, capacity)),
        KnowledgeInvocation::Change(change_id) => service
            .knowledge_change(context, change_id)
            .await
            .and_then(|value| crate::knowledge_output::change(value, capacity)),
        KnowledgeInvocation::Prepare(request) => service
            .knowledge_change_prepare(context, &request, &StaticPipelineDefinitions)
            .await
            .and_then(|value| crate::knowledge_output::prepare(value, capacity)),
        KnowledgeInvocation::Review(request) => service
            .knowledge_change_review(context, &request)
            .await
            .and_then(|value| crate::knowledge_output::review(value, capacity)),
        KnowledgeInvocation::Publish(request) => service
            .knowledge_change_publish(context, &request)
            .await
            .and_then(|value| crate::knowledge_output::publish(value, capacity)),
        KnowledgeInvocation::Refresh(request) => service
            .pipeline_knowledge_refresh(context, &request)
            .await
            .and_then(|value| crate::knowledge_output::refresh(value, capacity)),
    }
}
