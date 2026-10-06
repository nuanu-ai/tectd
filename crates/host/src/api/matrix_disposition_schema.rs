use super::catalog_support::{RouteSpec, uuid};
use crate::tools::object_schema;
use serde_json::json;

pub(super) fn routes(example_id: &str) -> Vec<RouteSpec> {
    vec![
        RouteSpec {
            tool: "command",
            route: "engineering.matrix.disposition.record",
            internal: "record_matrix_disposition",
            summary: "Record an explicit selected or blocked Matrix decision.",
            conditions: "Requires an authenticated Owner native session, exact task/revision/input/choice/opportunity bindings and current verification for selection or after_advice. No caller actor fields. Advice is not automatic selection.",
            effects: "Appends an immutable disposition only, not a planning selection link or caller effect.",
            retry: "Repeat only identical fields and request_id; read engineering.matrix.disposition.get after uncertainty.",
            schema: disposition_schema(object_schema(
                json!({
                    "request_id":uuid(),"task_id":uuid(),"expected_task_revision":{"type":"integer","minimum":1},
                    "expected_input_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "expected_choice_set_digest":{"type":["string","null"],"pattern":"^[0-9a-f]{64}$"},
                    "opportunity_id":uuid(),"basis":{"type":"string","enum":["after_advice","no_call","manual"]},
                    "advice_id":{"type":["string","null"],"format":"uuid"},
                    "advice_digest":{"type":["string","null"],"pattern":"^[0-9a-f]{64}$"},
                    "decision":{"oneOf":[
                        object_schema(json!({"outcome":{"const":"selected"},"selected_choice_id":{"type":"string","minLength":1,"maxLength":4096}}),json!(["outcome","selected_choice_id"])),
                        object_schema(json!({"outcome":{"const":"blocked"},"blocked_reason":{"type":"string","minLength":1,"maxLength":4096}}),json!(["outcome","blocked_reason"]))
                    ]}
                }),
                json!([
                    "request_id",
                    "task_id",
                    "expected_task_revision",
                    "expected_input_digest",
                    "expected_choice_set_digest",
                    "opportunity_id",
                    "basis",
                    "advice_id",
                    "advice_digest",
                    "decision"
                ]),
            )),
            example: json!({"request_id":example_id,"task_id":example_id,"expected_task_revision":1,"expected_input_digest":"a".repeat(64),"expected_choice_set_digest":"b".repeat(64),"opportunity_id":example_id,"basis":"no_call","advice_id":null,"advice_digest":null,"decision":{"outcome":"selected","selected_choice_id":"owner-choice"}}),
        },
        RouteSpec {
            tool: "query",
            route: "engineering.matrix.disposition.get",
            internal: "get_matrix_disposition",
            summary: "Read an immutable explicit Matrix disposition by task and request.",
            conditions: "Requires an authenticated Owner or independent Verifier session bound to this tenant/workspace. Owner read is creator-principal and session bound; Verifier read does not require the creator's session.",
            effects: "Reads typed decision and provenance without raw provider data. Read success is not currentness.",
            retry: "Safe to repeat. Missing or nonvisible exact task/request returns not_found.",
            schema: object_schema(
                json!({"task_id":uuid(),"request_id":uuid()}),
                json!(["task_id", "request_id"]),
            ),
            example: json!({"task_id":example_id,"request_id":example_id}),
        },
    ]
}

// These conditions mirror RecordMatrixDisposition::validate. A nullable field
// is allowed only in the combinations the existing domain accepts.
fn disposition_schema(mut schema: serde_json::Value) -> serde_json::Value {
    schema["allOf"] = json!([
        {"if":{"properties":{"decision":{"properties":{"outcome":{"const":"selected"}},"required":["outcome"]}},"required":["decision"]},
         "then":{"properties":{"expected_choice_set_digest":{"type":"string"}}}},
        {"if":{"properties":{"basis":{"const":"after_advice"}},"required":["basis"]},
         "then":{"properties":{"advice_id":{"type":"string"},"advice_digest":{"type":"string"}}}},
        {"if":{"properties":{"basis":{"enum":["no_call","manual"]}},"required":["basis"]},
         "then":{"properties":{"advice_id":{"type":"null"},"advice_digest":{"type":"null"}}}}
    ]);
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disposition_routes_append_only_two_internal_commands_to_five_tools() {
        let routes = routes("00000000-0000-4000-8000-000000000001");
        assert_eq!(routes.len(), 2);
        assert_eq!(
            (routes[0].tool, routes[0].route),
            ("command", "engineering.matrix.disposition.record")
        );
        assert_eq!(
            (routes[1].tool, routes[1].route),
            ("query", "engineering.matrix.disposition.get")
        );
        for route in routes {
            assert_eq!(route.schema["additionalProperties"], false);
            let properties = route.schema["properties"].as_object().unwrap();
            assert!(!properties.contains_key("actor_id"));
            assert!(!properties.contains_key("recorded_by_session_id"));
        }
    }
}
