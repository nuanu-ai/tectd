use super::catalog_support::{RouteSpec, uuid};
use crate::tools::object_schema;
use serde_json::json;

pub(super) fn routes(example_id: &str) -> Vec<RouteSpec> {
    let key = json!({"type":"string","minLength":1,"maxLength":256,"x-maxUtf8Bytes":256});
    let exact = object_schema(
        json!({"preparation_request_key":key}),
        json!(["preparation_request_key"]),
    );
    vec![
        RouteSpec {
            tool: "query",
            route: "model.route.host.selection",
            internal: "prepare_model_route_host_selection",
            summary: "Read current authenticated model-route source material for one explicit route selection.",
            conditions: "Requires current Owner/session/source, exact saved decision and accepted disposition, task, work-context, catalogue and input pins.",
            effects: "Read only; returns current source material, not portable authorization. An owned host must re-read through its authenticated socket before dispatch. No launcher or provider call.",
            retry: "Safe to repeat while all exact pins remain current.",
            schema: object_schema(
                json!({
                    "preparation_request_key":key,"decision_id":uuid(),"disposition_id":uuid(),
                    "expected_task_id":uuid(),"expected_task_revision":{"type":"integer","minimum":1},
                    "expected_work_context_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "expected_catalogue_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "selected_route_id":{"type":"string","minLength":1,"maxLength":128,"x-maxUtf8Bytes":128},
                    "input_sha256":{"type":"string","pattern":"^[0-9a-f]{64}$"},"invocation_key":key
                }),
                json!([
                    "preparation_request_key",
                    "decision_id",
                    "disposition_id",
                    "expected_task_id",
                    "expected_task_revision",
                    "expected_work_context_digest",
                    "expected_catalogue_digest",
                    "selected_route_id",
                    "input_sha256",
                    "invocation_key"
                ]),
            ),
            example: json!({"preparation_request_key":"route-1","decision_id":example_id,"disposition_id":example_id,
                "expected_task_id":example_id,"expected_task_revision":1,"expected_work_context_digest":"a".repeat(64),
                "expected_catalogue_digest":"b".repeat(64),"selected_route_id":"route-a","input_sha256":"c".repeat(64),"invocation_key":"invocation-1"}),
        },
        RouteSpec {
            tool: "command",
            route: "model.route.prepare",
            internal: "model_route_prepare",
            summary: "Prepare one saved Matrix-selected Work recommendation from typed caller and host-owned facts.",
            conditions: "Requires an authenticated Owner and current native session, exact Matrix disposition, saved Work revision, and policy catalogue. Unknown facts fail closed.",
            effects: "Persists an immutable recommendation-only preparation with requested, recommended and actual routes distinct; no adviser call or model execution.",
            retry: "Replay the same request key and exact inputs; changed material conflicts.",
            schema: object_schema(
                json!({
                    "disposition_id":uuid(),"expected_task_id":uuid(),
                    "expected_task_revision":{"type":"integer","minimum":1},
                    "expected_candidate_set_id":uuid(),"expected_caller_request_id":uuid(),
                    "expected_mapped_work_node_id":uuid(),
                    "expected_mapped_work_node_revision":{"type":"integer","minimum":1},
                    "request_key":{"type":"string","minLength":1,"maxLength":256,"x-maxUtf8Bytes":256},
                    "requested_route_id":{"type":"string","minLength":1,"maxLength":256},
                    "request_preference":{"type":"string","enum":["use_workspace","skip"],"default":"use_workspace"}
                }),
                json!([
                    "disposition_id",
                    "expected_task_id",
                    "expected_task_revision",
                    "expected_candidate_set_id",
                    "expected_caller_request_id",
                    "expected_mapped_work_node_id",
                    "expected_mapped_work_node_revision",
                    "request_key"
                ]),
            ),
            example: json!({"disposition_id":example_id,"expected_task_id":example_id,
                "expected_task_revision":1,"expected_candidate_set_id":example_id,
                "expected_caller_request_id":example_id,"expected_mapped_work_node_id":example_id,
                "expected_mapped_work_node_revision":1,"request_key":"route-1"}),
        },
        RouteSpec {
            tool: "command",
            route: "model.route.run",
            internal: "model_route_run",
            summary: "Attempt at most one optional Jev ranking of finite eligible route IDs.",
            conditions: "Requires the exact saved preparation key, current Owner session, Matrix/Work revision and unchanged host/catalogue facts. No caller ranking is accepted.",
            effects: "Commits a one-use send fence, seals raw response bytes before parsing, and records a recommendation-only decision or no-call. It never executes a recommended model.",
            retry: "An uncertain send is terminal: repeating this call only reads the durable state and never sends again.",
            schema: exact.clone(),
            example: json!({"preparation_request_key":"route-1"}),
        },
        RouteSpec {
            tool: "query",
            route: "model.route.get",
            internal: "model_route_get",
            summary: "Read the exact preparation, adviser attempt state, decision and disposition.",
            conditions: "Requires the authenticated Owner session and current Matrix-selected Work binding.",
            effects: "Reads requested, recommended and independently observed actual routes separately; absent execution evidence leaves actual null.",
            retry: "Safe to repeat; no provider or model call occurs.",
            schema: exact,
            example: json!({"preparation_request_key":"route-1"}),
        },
        RouteSpec {
            tool: "command",
            route: "model.route.disposition",
            internal: "model_route_disposition",
            summary: "Explicitly accept or reject one saved model-route recommendation.",
            conditions: "Requires an authenticated Owner session, current Matrix/Work context and a ranked saved decision.",
            effects: "Records an immutable accept/reject receipt; does not dispatch the route.",
            retry: "Exact disposition ID and payload replay; changed action or rationale conflicts.",
            schema: object_schema(
                json!({"disposition_id":uuid(),"decision_id":uuid(),
                "action":{"type":"string","enum":["accept","reject"]},
                "rationale":{"type":"string","minLength":1,"maxLength":4096}}),
                json!(["disposition_id", "decision_id", "action", "rationale"]),
            ),
            example: json!({"disposition_id":example_id,"decision_id":example_id,
                "action":"reject","rationale":"Use a different route"}),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_catalogue_has_five_typed_routes_and_no_caller_ranking() {
        let routes = routes("00000000-0000-4000-8000-000000000001");
        assert_eq!(
            routes.iter().map(|r| r.route).collect::<Vec<_>>(),
            [
                "model.route.host.selection",
                "model.route.prepare",
                "model.route.run",
                "model.route.get",
                "model.route.disposition",
            ]
        );
        for route in &routes {
            assert!(!route.schema.to_string().contains("ranked_route_ids"));
            assert!(!route.schema.to_string().contains("actual_route_id"));
            assert!(!route.schema.to_string().contains("session_preference"));
        }
        assert_eq!(
            routes[2].schema["required"],
            json!(["preparation_request_key"])
        );
        assert_eq!(routes[0].tool, "query");
        assert_eq!(routes[2].tool, "command");
        assert_eq!(routes[3].tool, "query");
    }
}
