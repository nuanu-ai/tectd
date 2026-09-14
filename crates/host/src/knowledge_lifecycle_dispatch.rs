use crate::knowledge_lifecycle_definitions::StaticKnowledgeLifecycleDefinitions;
use crate::knowledge_lifecycle_encoding::KnowledgeEncoding;
use crate::knowledge_lifecycle_tools::KnowledgeLifecycleInvocation;
use crate::pipeline_definitions::StaticPipelineDefinitions;
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, Result};

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: KnowledgeLifecycleInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    match invocation {
        KnowledgeLifecycleInvocation::Lifecycle(query) => {
            let guard = KnowledgeEncoding::lifecycle(capacity, query.clone());
            service
                .knowledge_lifecycle(context, &query, &guard)
                .await
                .and_then(|value| {
                    crate::knowledge_lifecycle_output::lifecycle(value, &query, capacity)
                })
        }
        KnowledgeLifecycleInvocation::Unit(query) => {
            let guard = KnowledgeEncoding::unit(capacity, query.clone());
            service
                .knowledge_unit(context, &query, &guard)
                .await
                .and_then(|value| crate::knowledge_lifecycle_output::unit(value, &query, capacity))
        }
        KnowledgeLifecycleInvocation::Begin(request) => {
            let guard = KnowledgeEncoding::new(capacity);
            service
                .knowledge_change_begin(
                    context,
                    &request,
                    &StaticKnowledgeLifecycleDefinitions,
                    &guard,
                )
                .await
                .and_then(|value| crate::knowledge_lifecycle_output::begin(value, capacity))
        }
        KnowledgeLifecycleInvocation::PhaseComplete(request) => {
            let guard = KnowledgeEncoding::new(capacity);
            service
                .knowledge_change_phase_complete(
                    context,
                    &request,
                    &StaticPipelineDefinitions,
                    &guard,
                )
                .await
                .and_then(|value| crate::knowledge_lifecycle_output::mutation(value, capacity))
        }
        KnowledgeLifecycleInvocation::RecordInput(request) => {
            let guard = KnowledgeEncoding::new(capacity);
            service
                .knowledge_change_record_input(context, &request, &guard)
                .await
                .and_then(|value| crate::knowledge_lifecycle_output::mutation(value, capacity))
        }
        KnowledgeLifecycleInvocation::Commit(request) => {
            let guard = KnowledgeEncoding::new(capacity);
            service
                .knowledge_change_commit(context, &request, &guard)
                .await
                .and_then(|value| crate::knowledge_lifecycle_output::commit(value, capacity))
        }
        KnowledgeLifecycleInvocation::Settle(request) => {
            let guard = KnowledgeEncoding::new(capacity);
            let change_id = request.change_id;
            service
                .knowledge_change_settle_effects(context, &request, &guard)
                .await
                .and_then(|value| {
                    crate::knowledge_lifecycle_output::settle(change_id, value, capacity)
                })
        }
    }
}
