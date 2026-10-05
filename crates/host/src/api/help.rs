use super::*;

pub(super) fn search(text: Option<&str>, tool_filter: Option<&str>) -> Value {
    let needle = text.unwrap_or_default().to_lowercase();
    let matches = |values: &[&str]| {
        needle.is_empty()
            || values
                .iter()
                .any(|value| value.to_lowercase().contains(&needle))
    };
    let mut hits = Vec::new();
    for tool in PUBLIC_TOOLS {
        let description = tool_summary(tool);
        if tool_filter.is_none_or(|filter| filter == tool) && matches(&[tool, description]) {
            hits.push(json!({"kind":"tool","tool":tool,"summary":description}));
        }
    }
    for spec in routes() {
        let mut values = vec![spec.tool, spec.route, spec.summary];
        values.extend_from_slice(spec.aliases());
        if tool_filter.is_none_or(|filter| filter == spec.tool) && matches(&values) {
            hits.push(
                json!({"kind":"route","tool":spec.tool,"route":spec.route,"summary":spec.summary}),
            );
        }
    }
    for (method, summary) in [
        (
            "tectd-program",
            "Method for forming and continuing a durable Program PRD.",
        ),
        (
            "tectd-setup",
            "Method for composing and safely applying initial workspace instructions.",
        ),
        (
            "tectd-scope-candidates",
            "Method for deriving, reviewing, and continuing durable Scope candidates.",
        ),
        (
            "tectd-slice-candidates",
            "Method for designing and reviewing a complete revisable Slice-candidate plan.",
        ),
    ] {
        if tool_filter.is_none_or(|filter| filter == "help") && matches(&[method, summary]) {
            hits.push(json!({"kind":"method","tool":"help","method":method,"summary":summary}));
        }
    }
    if tool_filter.is_none_or(|filter| filter == "help")
        && matches(&[
            "response-rules",
            "complete rules for replies and follow-up work",
        ])
    {
        hits.push(json!({"kind":"response_rules","tool":"help","text":"response-rules","summary":"Complete rules for replies and follow-up work."}));
    }
    let total_matches = hits.len();
    hits.truncate(HELP_LIMIT);
    let truncated = total_matches > hits.len();
    let refinement = if truncated {
        Some("Add text or a tool filter to narrow results.")
    } else if total_matches == 0 {
        Some("Try a different short task phrase or remove the tool filter.")
    } else {
        None
    };
    json!({"mode":"search","text":text,"tool":tool_filter,"hits":hits,
        "returned":hits.len(),"total_matches":total_matches,"truncated":truncated,
        "refine_search":refinement})
}

pub(super) fn describe_tool(tool: &str) -> Value {
    let routes: Vec<_> = routes()
        .iter()
        .filter(|spec| spec.tool == tool)
        .map(|spec| spec.route)
        .collect();
    let schema = definitions()["tools"]
        .as_array()
        .and_then(|tools| tools.iter().find(|entry| entry["name"] == tool))
        .map(|entry| entry["inputSchema"].clone())
        .unwrap_or(Value::Null);
    json!({"mode":"describe","kind":"tool","tool":tool,"description":tool_summary(tool),
        "input_schema":schema,"routes":routes})
}

pub(super) fn describe_route(spec: &RouteSpec) -> Value {
    json!({"mode":"describe","kind":"route","tool":spec.tool,"route":spec.route,
        "description":spec.summary,"params_schema":spec.schema,"conditions":spec.conditions,
        "effects":spec.effects,"retry":spec.retry,
        "example":{"tool":spec.tool,"arguments":{"route":spec.route,"params":spec.example}}})
}
