mod candidate_schema;
mod catalog;
mod catalog_aliases;
mod knowledge_lifecycle_schema;
mod knowledge_schema;
mod slice_schema;

use crate::tools::{annotations, object_schema};
use catalog::{RouteSpec, routes};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tect_domain::{Error, Result};

pub(crate) const WIRE_API_VERSION: u32 = 2;
pub(crate) const INVALID_PUBLIC_CALL: &str = "__invalid_public_api_v2_call__";
const HELP_LIMIT: usize = 25;
const PUBLIC_TOOLS: [&str; 5] = ["get_state", "query", "command", "execute", "help"];
const PROGRAM_METHOD: &str = include_str!("../../../skills/tectd-program/SKILL.md");
const SETUP_METHOD: &str = include_str!("../../../skills/tectd-setup/SKILL.md");
const SCOPE_CANDIDATE_METHOD: &str =
    include_str!("../../../skills/tectd-scope-candidates/SKILL.md");
const SLICE_CANDIDATE_METHOD: &str =
    include_str!("../../../skills/tectd-slice-candidates/SKILL.md");

#[derive(Debug)]
pub(crate) struct InternalCall {
    pub name: &'static str,
    pub arguments: Value,
}

#[derive(Clone)]
pub(crate) enum HelpRequest {
    Search {
        text: Option<String>,
        tool: Option<String>,
    },
    DescribeTool(String),
    DescribeRoute(RouteSpec),
    DescribeMethod(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutedArguments {
    route: String,
    params: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HelpArguments {
    mode: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    route: Option<String>,
    #[serde(default)]
    method: Option<String>,
}

pub(crate) fn definitions() -> Value {
    let route_enum = |tool: &str| {
        Value::Array(
            routes()
                .iter()
                .filter(|spec| spec.tool == tool)
                .map(|spec| json!(spec.route))
                .collect(),
        )
    };
    let routed = |tool: &str| {
        object_schema(
            json!({
                "route":{"type":"string","enum":route_enum(tool)},
                "params":{"type":"object"}
            }),
            json!(["route", "params"]),
        )
    };
    json!({"tools":[
        tool_definition("get_state", "Read bounded state for this native session without filesystem, Git, or writes.", object_schema(json!({}), json!([])), true, true),
        tool_definition("query", "Run one named read-only TectD route. Use help to inspect its exact parameter contract.", routed("query"), true, true),
        tool_definition("command", "Run one named logical state transition. Use help to inspect its exact parameter contract.", routed("command"), false, false),
        tool_definition("execute", "Run one named explicit external effect. Only setup.apply is currently supported.", routed("execute"), false, true),
        tool_definition("help", "Search or describe the bounded TectD API and its four embedded methods.", help_schema(), true, true)
    ]})
}

fn tool_definition(
    name: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
    idempotent: bool,
) -> Value {
    let mut hints = annotations(read_only);
    hints["idempotentHint"] = json!(idempotent);
    json!({"name":name,"description":description,"inputSchema":input_schema,"annotations":hints})
}

fn help_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "mode":{"type":"string","enum":["search","describe"]},
            "text":{"type":"string"},
            "tool":{"type":"string","enum":PUBLIC_TOOLS},
            "route":{"type":"string"},
            "method":{"type":"string","enum":["tectd-program","tectd-setup","tectd-scope-candidates","tectd-slice-candidates"]}
        },
        "required":["mode"],
        "additionalProperties":false,
        "oneOf":[
            {
                "properties":{"mode":{"const":"search"}},
                "not":{"anyOf":[{"required":["route"]},{"required":["method"]}]}
            },
            {
                "properties":{"mode":{"const":"describe"}},
                "required":["tool"],
                "not":{"anyOf":[{"required":["text"]},{"required":["route"]},{"required":["method"]}]}
            },
            {
                "properties":{"mode":{"const":"describe"},"tool":{"enum":["query","command","execute"]}},
                "required":["tool","route"],
                "not":{"anyOf":[{"required":["text"]},{"required":["method"]}]}
            },
            {
                "properties":{"mode":{"const":"describe"}},
                "required":["method"],
                "not":{"anyOf":[{"required":["text"]},{"required":["tool"]},{"required":["route"]}]}
            }
        ]
    })
}

pub(crate) fn decode_public_call(name: &str, arguments: Value) -> Result<InternalCall> {
    match name {
        "get_state" if empty_object(&arguments) => validate_internal("get_state", arguments),
        "query" | "command" | "execute" => {
            let routed: RoutedArguments =
                serde_json::from_value(arguments).map_err(|_| Error::InvalidArguments)?;
            if !routed.params.is_object() {
                return Err(Error::InvalidArguments);
            }
            let spec = route_for(name, &routed.route).ok_or(Error::InvalidArguments)?;
            validate_internal(spec.internal, routed.params)
        }
        "help" => {
            parse_help(arguments.clone())?;
            Ok(InternalCall {
                name: "help",
                arguments,
            })
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn validate_internal(name: &'static str, arguments: Value) -> Result<InternalCall> {
    crate::tools::parse_invocation(name, arguments.clone())?;
    Ok(InternalCall { name, arguments })
}

pub(crate) fn parse_help(arguments: Value) -> Result<HelpRequest> {
    for field in ["text", "tool", "route", "method"] {
        if arguments.get(field).is_some_and(Value::is_null) {
            return Err(Error::InvalidArguments);
        }
    }
    let args: HelpArguments =
        serde_json::from_value(arguments).map_err(|_| Error::InvalidArguments)?;
    if args.text.as_ref().is_some_and(|text| text.contains('\0'))
        || args
            .tool
            .as_ref()
            .is_some_and(|tool| !PUBLIC_TOOLS.contains(&tool.as_str()))
        || args.method.as_ref().is_some_and(|method| {
            !matches!(
                method.as_str(),
                "tectd-program"
                    | "tectd-setup"
                    | "tectd-scope-candidates"
                    | "tectd-slice-candidates"
            )
        })
    {
        return Err(Error::InvalidArguments);
    }
    match args.mode.as_str() {
        "search" if args.route.is_none() && args.method.is_none() => Ok(HelpRequest::Search {
            text: args.text,
            tool: args.tool,
        }),
        "describe" if args.text.is_none() => match (args.tool, args.route, args.method) {
            (Some(tool), None, None) => Ok(HelpRequest::DescribeTool(tool)),
            (Some(tool), Some(route), None)
                if matches!(tool.as_str(), "query" | "command" | "execute") =>
            {
                Ok(HelpRequest::DescribeRoute(
                    route_for(&tool, &route).ok_or(Error::InvalidArguments)?,
                ))
            }
            (None, None, Some(method)) => Ok(HelpRequest::DescribeMethod(method)),
            _ => Err(Error::InvalidArguments),
        },
        _ => Err(Error::InvalidArguments),
    }
}

pub(crate) fn help(request: HelpRequest) -> Result<Value> {
    Ok(match request {
        HelpRequest::Search { text, tool } => search(text.as_deref(), tool.as_deref()),
        HelpRequest::DescribeTool(tool) => describe_tool(&tool),
        HelpRequest::DescribeRoute(spec) => describe_route(&spec),
        HelpRequest::DescribeMethod(method) => {
            let (description, body) = match method.as_str() {
                "tectd-program" => (
                    "Method for forming and continuing a durable Program PRD.",
                    PROGRAM_METHOD,
                ),
                "tectd-setup" => (
                    "Method for composing and safely applying initial workspace instructions.",
                    SETUP_METHOD,
                ),
                "tectd-scope-candidates" => (
                    "Method for deriving, reviewing, and continuing durable Scope candidates.",
                    SCOPE_CANDIDATE_METHOD,
                ),
                "tectd-slice-candidates" => (
                    "Method for designing and reviewing a complete revisable Slice-candidate plan.",
                    SLICE_CANDIDATE_METHOD,
                ),
                _ => return Err(Error::InternalInvariant),
            };
            let mut value = json!({"mode":"describe","kind":"method","method":method,
                "tool":"help","description":description,"body":body});
            if method == "tectd-scope-candidates" {
                value["method_revision"] = json!(crate::scope_guidance::METHOD_REVISION);
                value["guidance_registry"] = crate::scope_guidance::help_registry()?;
            }
            if method == "tectd-slice-candidates" {
                value["method_revision"] = json!(crate::slice_guidance::METHOD_REVISION);
                let details = crate::slice_guidance::help()?;
                value["guidance_registry"] = details["guidance_registry"].clone();
                value["pipeline_catalog"] = details["pipeline_catalog"].clone();
            }
            value
        }
    })
}

fn search(text: Option<&str>, tool_filter: Option<&str>) -> Value {
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

fn describe_tool(tool: &str) -> Value {
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

fn describe_route(spec: &RouteSpec) -> Value {
    json!({"mode":"describe","kind":"route","tool":spec.tool,"route":spec.route,
        "description":spec.summary,"params_schema":spec.schema,"conditions":spec.conditions,
        "effects":spec.effects,"retry":spec.retry,
        "example":{"tool":spec.tool,"arguments":{"route":spec.route,"params":spec.example}}})
}

fn tool_summary(tool: &str) -> &'static str {
    match tool {
        "get_state" => "Read bounded DB-only state for the current native session.",
        "query" => "Run one of twelve named read-only routes.",
        "command" => "Run one of twenty-eight named logical state-transition routes.",
        "execute" => "Run the single explicit external-effect route setup.apply.",
        "help" => "Search or describe this API and its four embedded methods.",
        _ => "",
    }
}

fn route_for(tool: &str, route: &str) -> Option<RouteSpec> {
    routes()
        .iter()
        .find(|spec| spec.tool == tool && spec.route == route)
        .cloned()
}

fn route_for_internal(internal: &str) -> Option<RouteSpec> {
    routes()
        .iter()
        .find(|spec| spec.internal == internal)
        .cloned()
}

pub(crate) fn ready_action(internal: &str, params: Value) -> Result<Value> {
    let (tool, arguments) = public_call(internal, params)?;
    decode_public_call(tool, arguments.clone()).map_err(|_| Error::InternalInvariant)?;
    Ok(json!({"kind":"ready_call","tool":tool,"arguments":arguments}))
}

pub(crate) fn method_action(method: &str) -> Result<Value> {
    let arguments = json!({"mode":"describe","method":method});
    decode_public_call("help", arguments.clone()).map_err(|_| Error::InternalInvariant)?;
    Ok(json!({"kind":"ready_call","tool":"help","arguments":arguments}))
}

pub(crate) fn needs_action(
    kind: &str,
    internal: &str,
    params: Value,
    descriptor_name: &str,
    descriptor: Value,
) -> Result<Value> {
    if !matches!(kind, "needs_input" | "needs_context") {
        return Err(Error::InternalInvariant);
    }
    let (tool, arguments) = public_call(internal, params)?;
    let properties = if tool == "get_state" {
        Some(std::collections::BTreeSet::new())
    } else {
        route_for_internal(internal).and_then(|spec| allowed_properties(&spec.schema))
    };
    let known = arguments
        .get("params")
        .and_then(Value::as_object)
        .ok_or(Error::InternalInvariant)?;
    if properties.is_none_or(|allowed| known.keys().any(|key| !allowed.contains(key))) {
        return Err(Error::InternalInvariant);
    }
    let mut action = json!({"kind":kind,"tool":tool,"arguments":arguments});
    action[descriptor_name] = descriptor;
    Ok(action)
}

fn allowed_properties(schema: &Value) -> Option<std::collections::BTreeSet<String>> {
    if let Some(properties) = schema["properties"].as_object() {
        return Some(properties.keys().cloned().collect());
    }
    let variants = schema["oneOf"].as_array()?;
    let mut names = std::collections::BTreeSet::new();
    for variant in variants {
        names.extend(allowed_properties(variant)?);
    }
    Some(names)
}

fn public_call(internal: &str, params: Value) -> Result<(&'static str, Value)> {
    if internal == "get_state" {
        return Ok(("get_state", params));
    }
    if internal == "help" {
        return Ok(("help", params));
    }
    let spec = route_for_internal(internal).ok_or(Error::InternalInvariant)?;
    Ok((spec.tool, json!({"route":spec.route,"params":params})))
}

fn empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(Map::is_empty)
}

#[cfg(test)]
mod tests;
