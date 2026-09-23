use super::catalog_support::{RouteSpec, uuid};
use crate::tools::object_schema;
use serde_json::{Value, json};

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

pub(super) fn routes(example_id: &str) -> Vec<RouteSpec> {
    vec![
        route!(
            "query",
            "workspace.advisory.config",
            "get_advisory_config",
            "Read the workspace Jev advisory mode and its revision.",
            "Requires an authenticated open native session. A missing row is the explicit default: disabled at revision 0.",
            "Reads workspace-scoped advisory configuration only; it never contacts a provider.",
            "Safe to repeat.",
            object_schema(json!({}), json!([])),
            json!({}),
        ),
        route!(
            "command",
            "workspace.advisory.configure",
            "configure_advisory",
            "Set workspace advisory mode and non-secret provider/model references with owner-only compare-and-set revision.",
            "Requires an authenticated open native session whose principal is the tenant owner. The first write expects revision 0; later writes must name the current revision.",
            "Appends config history and changes only future dispatch authorization. In-flight sends are not promised to cancel.",
            "Retry only with the same expected revision and mode; a stale revision must be re-read.",
            advisory_config_schema(),
            json!({"expected_revision":0,"mode":"optional","provider_profile_ref":{"id":"jev-production"},"model_configuration":{"model":"jev-advisory-v1"}}),
        ),
        route!(
            "query",
            "workspace.advisory.audit",
            "workspace_advisory_audit",
            "Read a stable page of workspace advisory opportunities, dispatch facts and exact basic aggregates.",
            "Requires an authenticated open native session in the workspace. Optional scope UUID and closed-enum filters are tenant-bound; after is the last opportunity ID returned by the preceding page.",
            "Returns metadata and byte counts, never request/response bodies or credentials. Unknown send/token facts remain explicit, and future advice/disposition/caller/verifier links are null.",
            "Safe to repeat with the same filters and cursor.",
            advisory_audit_schema(false),
            json!({"limit":50}),
        ),
        route!(
            "query",
            "scope.advisory.get",
            "scope_advisory_get",
            "Read one exact scope advisory opportunity and all of its ordered dispatch facts.",
            "Requires an authenticated open native session and an existing scope in that workspace; the opportunity must belong to that exact scope.",
            "Returns metadata, retry lineage, nullable measurements and raw-response reference only. It does not return raw provider bodies and does not infer future links.",
            "Safe to repeat.",
            object_schema(
                json!({"scope_id":uuid(),"opportunity_id":uuid()}),
                json!(["scope_id", "opportunity_id"]),
            ),
            json!({"scope_id":example_id,"opportunity_id":example_id}),
        ),
        route!(
            "query",
            "scope.advisory.audit",
            "scope_advisory_audit",
            "Read a stable filtered page of advisory opportunities and dispatch facts for one exact scope.",
            "Requires an authenticated open native session and an existing scope in that workspace. Optional filters are closed enums; after is the preceding page's last opportunity ID.",
            "Returns exact no-call reasons, send uncertainty and aggregates without raw bodies or inferred disposition/caller/verifier outcomes.",
            "Safe to repeat with the same filters and cursor.",
            advisory_audit_schema(true),
            json!({"scope_id":example_id,"limit":50}),
        ),
        route!(
            "query",
            "candidate.advisory.get",
            "candidate_advisory_get",
            "Read one candidate-set advisory opportunity and its ordered dispatch facts.",
            "Requires an authenticated open native session and a candidate set in the workspace; the opportunity must target that exact candidate set.",
            "Returns metadata and dispatch facts without request or response bodies.",
            "Safe to repeat.",
            object_schema(
                json!({"candidate_set_id":uuid(),"opportunity_id":uuid()}),
                json!(["candidate_set_id", "opportunity_id"])
            ),
            json!({"candidate_set_id":example_id,"opportunity_id":example_id}),
        ),
        route!(
            "query",
            "candidate.advisory.audit",
            "candidate_advisory_audit",
            "Read a filtered page of opportunities, dispatch facts and aggregates for one candidate set.",
            "Requires an authenticated open native session and a candidate set in the workspace; after is the preceding page's last opportunity ID.",
            "Returns exact counts and no-call reasons without raw provider bodies.",
            "Safe to repeat with the same filters and cursor.",
            advisory_candidate_audit_schema(),
            json!({"candidate_set_id":example_id,"limit":50}),
        ),
    ]
}

fn advisory_candidate_audit_schema() -> Value {
    let mut schema = advisory_audit_schema(false);
    let properties = schema["properties"]
        .as_object_mut()
        .expect("object schema properties");
    properties.remove("scope_id");
    properties.insert("candidate_set_id".into(), uuid());
    schema["required"] = json!(["candidate_set_id", "limit"]);
    schema
}

fn advisory_config_schema() -> Value {
    let provider_profile_ref =
        object_schema(json!({"id": advisory_identifier_schema()}), json!(["id"]));
    let model_configuration = object_schema(
        json!({"model": advisory_identifier_schema()}),
        json!(["model"]),
    );
    let mut schema = object_schema(
        json!({
            "expected_revision":{"type":"integer","minimum":0},
            "mode":{"type":"string","enum":["disabled","optional"]},
            "provider_profile_ref":{"oneOf":[provider_profile_ref,{"type":"null"}]},
            "model_configuration":{"oneOf":[model_configuration,{"type":"null"}]}
        }),
        json!(["expected_revision", "mode"]),
    );
    schema["oneOf"] = json!([
        {"required":["provider_profile_ref", "model_configuration"]},
        {"not":{"anyOf":[
            {"required":["provider_profile_ref"]},
            {"required":["model_configuration"]}
        ]}}
    ]);
    schema
}

fn advisory_identifier_schema() -> Value {
    json!({
        "type":"string",
        "minLength":1,
        "maxLength":256,
        "pattern":r"^[^\s\u0000](?:[^\u0000]*[^\s\u0000])?$"
    })
}

fn advisory_audit_schema(scope: bool) -> Value {
    let mut properties = serde_json::Map::from_iter([
        (
            "limit".into(),
            json!({"type":"integer","minimum":1,"maximum":100}),
        ),
        ("after".into(), uuid()),
        (
            "capability".into(),
            json!({"type":"string","enum":["scope_decomposition","engineering_profile","pipeline_recommendation","anti_bloat","model_routing"]}),
        ),
        (
            "decision_point".into(),
            json!({"type":"string","enum":["scope.decomposition.before_selection"]}),
        ),
        (
            "reason".into(),
            json!({"type":"string","enum":["workspace_disabled","session_skip","request_skip","deterministic_input_invalid","capability_unavailable","provider_unconfigured","configuration_changed","dispatch_authorized","provider_response","provider_failure","send_unknown"]}),
        ),
        (
            "state".into(),
            json!({"type":"string","enum":["prepared","no_call","awaiting_response","advised","invalidated","failed","unresolved"]}),
        ),
    ]);
    let required = if scope {
        properties.insert("scope_id".into(), uuid());
        json!(["scope_id", "limit"])
    } else {
        properties.insert("scope_id".into(), uuid());
        json!(["limit"])
    };
    object_schema(Value::Object(properties), required)
}
