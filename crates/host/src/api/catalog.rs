pub(crate) use super::catalog_support::RouteSpec;
use super::catalog_support::{nullable_text, page_limit, text, uuid};
use super::{
    candidate_schema, knowledge_lifecycle_schema, knowledge_maintenance_schema, knowledge_schema,
    knowledge_search_schema, matrix_task_schema, slice_schema,
};
use crate::tools::object_schema;
use serde_json::json;
use std::sync::OnceLock;
use tect_domain::{MAX_SOURCE_PATH_BYTES, MAX_WORKTREES};

macro_rules! route {
    ($tool:expr, $name:expr, $internal:expr, $summary:expr, $conditions:expr,
     $effects:expr, $retry:expr, $schema:expr, $example:expr $(,)?) => {
        RouteSpec {
            tool: $tool,
            route: $name,
            internal: $internal,
            summary: $summary,
            conditions: $conditions,
            effects: $effects,
            retry: $retry,
            schema: $schema,
            example: $example,
        }
    };
}

pub(crate) fn routes() -> &'static [RouteSpec] {
    static ROUTES: OnceLock<Vec<RouteSpec>> = OnceLock::new();
    ROUTES.get_or_init(build_routes)
}

fn build_routes() -> Vec<RouteSpec> {
    let example_id = "00000000-0000-4000-8000-000000000001";
    let mut routes: Vec<RouteSpec> = include!("catalog/query_program.rs");
    routes.extend(include!("catalog/remaining.rs"));
    routes.extend(knowledge_schema::routes(example_id));
    routes.extend(knowledge_lifecycle_schema::routes(example_id));
    routes.extend(knowledge_maintenance_schema::routes(example_id));
    routes.push(knowledge_search_schema::route());
    routes.extend(super::advisory_schema::routes(example_id));
    routes.extend(super::model_route_schema::routes(example_id));
    routes.extend([
        route!("command", "slice.pipeline.evidence_artifact.register", "slice_pipeline_evidence_artifact_register", "Register immutable evidence metadata and receive a backend-issued artifact identity.", "Requires an authenticated open native session; the supplied digest, size and provenance describe the exact future bytes.", "Creates an uploading artifact revision; no evidence is ready until finalize succeeds.", "The same request replays the same artifact; changed payload conflicts.", object_schema(json!({"request_id":uuid(),"digest":{"type":"string","pattern":"^[0-9a-fA-F]{64}$"},"size":{"type":"integer","minimum":0},"format":text(),"provenance":text(),"target":text()}), json!(["request_id","digest","size","format","provenance","target"])), json!({"request_id":example_id,"digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","size":12,"format":"text/plain","provenance":"operator-observed","target":"slice"})),
        route!("command", "slice.pipeline.evidence_artifact.finalize", "slice_pipeline_evidence_artifact_finalize", "Finalize one registered evidence artifact by hashing and sizing the submitted bytes.", "Requires an uploading artifact revision.", "Marks the immutable revision ready only when digest and byte size match; mismatches are durably rejected.", "The same request replays the same final state.", object_schema(json!({"request_id":uuid(),"artifact_id":uuid(),"revision":{"type":"integer","minimum":1},"body":{"type":"string"}}), json!(["request_id","artifact_id","revision","body"])), json!({"request_id":example_id,"artifact_id":example_id,"revision":1,"body":"exact evidence bytes"})),
        route!("query", "slice.pipeline.evidence_artifact.read", "slice_pipeline_evidence_artifact_read", "Read one bounded artifact fragment with explicit completeness and continuation cursor.", "Requires an accessible artifact revision and a positive limit no larger than 65536.", "Reads a bounded fragment and always returns complete plus next_offset when more bytes remain.", "Safe to repeat with the same offset and limit.", object_schema(json!({"artifact_id":uuid(),"revision":{"type":"integer","minimum":1},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":65536}}), json!(["artifact_id","revision"])), json!({"artifact_id":example_id,"revision":1,"offset":0,"limit":65536})),
    ]);
    routes
}

#[cfg(test)]
mod tests {
    use super::routes;

    #[test]
    fn split_preserves_original_route_names_and_order() {
        let expected: Vec<_> = include_str!("catalog/route_order_golden.txt")
            .lines()
            .collect();
        assert_eq!(expected.len(), 52);
        let actual: Vec<_> = routes()
            .iter()
            .take(expected.len())
            .map(|spec| format!("{} {} {}", spec.tool, spec.route, spec.internal))
            .collect();
        assert_eq!(actual, expected);
    }
}
