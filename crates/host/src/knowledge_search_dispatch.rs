use crate::knowledge_search_output::KnowledgeSearchEncoding;
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{KnowledgeSearchQuery, RequestContext, Result};

pub(crate) async fn execute(
    context: &RequestContext,
    query: KnowledgeSearchQuery,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let guard = KnowledgeSearchEncoding::new(capacity);
    service
        .knowledge_search(context, &query, &guard)
        .await
        .and_then(|value| crate::knowledge_search_output::encode(value, capacity))
}
