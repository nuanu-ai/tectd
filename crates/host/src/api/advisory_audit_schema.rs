use super::*;

pub(super) fn advisory_audit_schema(scope: bool) -> Value {
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
            json!({"type":"string","enum":["scope.decomposition.before_selection","engineering.profile.before_selection"]}),
        ),
        (
            "reason".into(),
            json!({"type":"string","enum":["workspace_disabled","session_skip","request_skip","choice_set_not_applicable","deterministic_input_invalid","capability_unavailable","provider_unconfigured","budget_policy_invalid","configuration_changed","dispatch_authorized","provider_response","provider_failure","send_unknown"]}),
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
