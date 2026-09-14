use crate::knowledge_lifecycle_definitions::StaticKnowledgeLifecycleDefinitions;
use crate::knowledge_maintenance_output::KnowledgeMaintenanceEncoding;
use crate::knowledge_maintenance_tools::KnowledgeMaintenanceInvocation;
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, Result};

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: KnowledgeMaintenanceInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    match invocation {
        KnowledgeMaintenanceInvocation::Query(query) => service
            .knowledge_maintenance(context, &query, &StaticKnowledgeLifecycleDefinitions)
            .await
            .and_then(|value| crate::knowledge_maintenance_output::query(value, &query, capacity)),
        KnowledgeMaintenanceInvocation::Observe(request) => {
            let guard = KnowledgeMaintenanceEncoding::new(capacity);
            service
                .observe_knowledge_maintenance(context, &request, &guard)
                .await
                .and_then(|value| crate::knowledge_maintenance_output::observe(value, capacity))
        }
        KnowledgeMaintenanceInvocation::Begin(request) => {
            let guard = KnowledgeMaintenanceEncoding::new(capacity);
            service
                .begin_knowledge_maintenance_change(
                    context,
                    &request,
                    &StaticKnowledgeLifecycleDefinitions,
                    &guard,
                )
                .await
                .and_then(|value| crate::knowledge_maintenance_output::begin(value, capacity))
        }
    }
}
