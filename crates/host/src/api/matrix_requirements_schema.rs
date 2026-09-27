use super::catalog_support::{RouteSpec, uuid};
use crate::tools::object_schema;
use serde_json::{Value, json};

pub(crate) fn locator() -> Value {
    json!({"oneOf":[
        object_schema(json!({"level":{"const":"program"},"program_id":uuid()}),json!(["level","program_id"])),
        object_schema(json!({"level":{"const":"scope"},"program_id":uuid(),"scope_id":uuid()}),json!(["level","program_id","scope_id"])),
        object_schema(json!({"level":{"const":"slice"},"program_id":uuid(),"scope_id":uuid(),"candidate_set_id":uuid(),"work_candidate_id":uuid(),"expected_work_revision":{"type":"integer","minimum":1}}),json!(["level","program_id","scope_id","candidate_set_id","work_candidate_id","expected_work_revision"])),
        object_schema(json!({"level":{"const":"opened_slice"},"slice_id":uuid()}),json!(["level","slice_id"]))
    ]})
}
fn patches() -> Value {
    let text = json!({"type":"string","minLength":1,"maxLength":256});
    let valued = |kind: &str, value: Value| {
        object_schema(
            json!({"kind":{"const":kind},"value":value}),
            json!(["kind", "value"]),
        )
    };
    let intent = json!({"oneOf":[object_schema(json!({"kind":{"const":"production_hotfix"}}),json!(["kind"])),object_schema(json!({"kind":{"const":"other"},"description":text}),json!(["kind","description"]))]});
    let value = json!({"oneOf":[valued("mode",json!({"type":"string","enum":["demo","mvp","production"]})),valued("intent",intent),valued("urgency",text.clone()),valued("promised_behavior",text.clone()),valued("promised_proof",text),object_schema(json!({"kind":{"const":"no_demand_commitment"}}),json!(["kind"])),object_schema(json!({"kind":{"const":"no_latency_commitment"}}),json!(["kind"]))]});
    json!({"type":"array","minItems":1,"maxItems":7,"items":{"oneOf":[object_schema(json!({"operation":{"const":"set"},"value":value}),json!(["operation","value"])),object_schema(json!({"operation":{"const":"remove"},"path":{"type":"string","enum":["mode","intent","urgency","promised_behavior","promised_proof","demand_commitment","latency_commitment"]}}),json!(["operation","path"]))]}})
}
pub(super) fn routes(example_id: &str) -> Vec<RouteSpec> {
    let loc = json!({"level":"program","program_id":example_id});
    vec![
        RouteSpec {
            tool: "command",
            route: "engineering.matrix.context.propose",
            internal: "matrix_context_propose",
            summary: "Propose declared mode and key requirements at a Program, accepted Scope or logical saved Work.",
            conditions: "Requires current Owner permission and actual anchor ancestry/ACL; expected context revision is exact.",
            effects: "Appends a pending proposal; accepted descendant values do not change before explicit owner confirmation.",
            retry: "Same request ID and payload replay; changed payload conflicts.",
            schema: object_schema(
                json!({"request_id":uuid(),"locator":locator(),"expected_context_revision":{"type":"integer","minimum":0},"patches":patches()}),
                json!([
                    "request_id",
                    "locator",
                    "expected_context_revision",
                    "patches"
                ]),
            ),
            example: json!({"request_id":example_id,"locator":loc,"expected_context_revision":0,"patches":[{"operation":"set","value":{"kind":"mode","value":"mvp"}}]}),
        },
        RouteSpec {
            tool: "command",
            route: "engineering.matrix.context.confirm",
            internal: "matrix_context_confirm",
            summary: "Record an owner's explicit response to one exact declared-requirements proposal.",
            conditions: "The owner must actually answer the exact proposal first; current Owner permission and anchor ACL are checked. An opaque response reference does not prove human authorship.",
            effects: "Appends immutable confirmation of the exact head revision and digest; inherits accepted declarations into descendants.",
            retry: "Same request ID and payload replay; changed confirmation conflicts.",
            schema: object_schema(
                json!({"request_id":uuid(),"locator":locator(),"proposal_revision":{"type":"integer","minimum":1},"proposal_digest":{"type":"string","pattern":"^[0-9a-f]{64}$"},"owner_response_ref":{"type":"string","minLength":1,"maxLength":256}}),
                json!([
                    "request_id",
                    "locator",
                    "proposal_revision",
                    "proposal_digest",
                    "owner_response_ref"
                ]),
            ),
            example: json!({"request_id":example_id,"locator":loc,"proposal_revision":1,"proposal_digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","owner_response_ref":"owner-response:exact-proposal"}),
        },
        RouteSpec {
            tool: "query",
            route: "engineering.matrix.context.effective.get",
            internal: "matrix_context_effective_get",
            summary: "Read effective confirmed declarations and source bindings for actual Program/Scope/Work ancestry.",
            conditions: "Requires authenticated workspace membership and anchor ACL. Opened Slice resolves its exact saved opening origin.",
            effects: "Read only; pending proposals are ignored and no snapshot or approval is appended.",
            retry: "Safe to repeat.",
            schema: object_schema(json!({"locator":locator()}), json!(["locator"])),
            example: json!({"locator":loc}),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_family_is_strict_and_keeps_five_owner_surface() {
        let routes = routes("00000000-0000-4000-8000-000000000001");
        assert_eq!(routes.len(), 3);
        for route in routes {
            assert!(route.route.starts_with("engineering.matrix.context."));
            assert_eq!(route.schema["additionalProperties"], false);
            assert!(route.tool == "command" || route.tool == "query");
        }
    }

    #[test]
    fn advertised_declaration_text_bounds_match_domain_limit() {
        fn max_lengths(value: &Value, found: &mut Vec<u64>) {
            if let Some(object) = value.as_object() {
                if let Some(maximum) = object.get("maxLength").and_then(Value::as_u64) {
                    found.push(maximum);
                }
                for child in object.values() {
                    max_lengths(child, found);
                }
            } else if let Some(array) = value.as_array() {
                for child in array {
                    max_lengths(child, found);
                }
            }
        }

        let routes = routes("00000000-0000-4000-8000-000000000001");
        let propose = routes
            .iter()
            .find(|route| route.route == "engineering.matrix.context.propose")
            .unwrap();
        let confirm = routes
            .iter()
            .find(|route| route.route == "engineering.matrix.context.confirm")
            .unwrap();
        let mut proposal_bounds = Vec::new();
        max_lengths(&propose.schema, &mut proposal_bounds);
        let mut confirmation_bounds = Vec::new();
        max_lengths(&confirm.schema, &mut confirmation_bounds);

        assert_eq!(proposal_bounds, vec![256; 4]);
        assert_eq!(confirmation_bounds, vec![256]);
    }
}
