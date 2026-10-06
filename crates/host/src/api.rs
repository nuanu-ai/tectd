mod help_requests;
pub(crate) use help_requests::{help, help_arguments, parse_help};
mod candidate_schema;
mod catalog;
mod catalog_aliases;
mod catalog_support;
mod help;
mod knowledge_lifecycle_schema;
mod knowledge_maintenance_schema;
mod knowledge_schema;
mod knowledge_search_schema;
mod slice_schema;

use crate::tools::{annotations, object_schema};
use catalog::{RouteSpec, routes};
use help::{describe_route, describe_tool, search};
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

fn planning_task_context() -> Value {
    let iris = json!({"type":"array","items":{"type":"string","minLength":1,"maxLength":4096,"pattern":"^(https?://|urn:)"},"maxItems":128,"uniqueItems":true});
    let classes = json!({"type":"array","items":{"type":"string","minLength":1,"maxLength":1024},"maxItems":128,"uniqueItems":true});
    object_schema(
        json!({"target_iris":iris.clone(),"environment_iris":iris,"action_classes":classes}),
        json!([]),
    )
}

fn planning_manifest_guard() -> Value {
    object_schema(
        json!({"manifest_id":{"type":"string","format":"uuid"},"digest":{"type":"string","minLength":1,"maxLength":256},"workspace_generation":{"type":"integer","minimum":0}}),
        json!(["manifest_id", "digest", "workspace_generation"]),
    )
}

#[derive(Debug)]
pub(crate) struct InternalCall {
    pub name: &'static str,
    pub arguments: Value,
}

#[derive(Clone)]
pub(crate) enum HelpRequest {
    Window {
        request: Box<HelpRequest>,
        arguments: Value,
        window: crate::planning_read::Window,
    },
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
        tool_definition("help", "Search or describe the bounded TectD API and its four embedded methods. Read complete response rules with text=response-rules.", help_schema(), true, true)
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
    let mut schema = json!({
        "type":"object",
        "properties":{
            "mode":{"type":"string","enum":["search","describe"]},
            "text":{"type":"string"},
            "tool":{"type":"string","enum":PUBLIC_TOOLS},
            "route":{"type":"string"},
            "method":{"type":"string","enum":["tectd-program","tectd-setup","tectd-scope-candidates","tectd-slice-candidates"]}
        },
        "anyOf":[{"required":["mode"]},{"properties":{"text":{"const":"response-rules"}},"required":["text"]}],
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
    });
    crate::planning_read::add_schema(&mut schema);
    schema
}

pub(crate) fn decode_public_call(name: &str, arguments: Value) -> Result<InternalCall> {
    match name {
        "get_state" if empty_object(&arguments) => validate_internal("get_state", arguments),
        "query" | "command" | "execute" => {
            let routed: RoutedArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
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
    // Phase completion has two backend-issued receipt fields that legacy
    // pinned definitions still put on the wire, while the current public
    // schema intentionally omits them. Canonicalize this one route at the
    // public bridge boundary so only those two known compatibility fields can
    // cross the second (daemon-side) strict decode. The definition-aware
    // validator remains responsible for accepting legacy receipts or refusing
    // caller-supplied v0.7 proof.
    if name == "slice_pipeline_phase_complete" {
        let arguments =
            crate::pipeline_tools::normalize_complete_arguments(arguments).map_err(|error| {
                error.normalize_pipeline_refusal(
                    "WP6-SCHEMA-COMPLETE-01",
                    "arguments.params",
                    "arguments matching the selected pipeline route schema",
                    "read_schema_and_retry",
                    "valid_pipeline_arguments",
                )
            })?;
        crate::tools::parse_invocation(name, arguments.clone())?;
        return Ok(InternalCall { name, arguments });
    }
    crate::tools::parse_invocation(name, arguments.clone())?;
    Ok(InternalCall { name, arguments })
}

#[cfg(test)]
pub(crate) fn attach_route_contract(action: &mut Value) -> Result<()> {
    let Some(tool) = action["tool"].as_str() else {
        return Ok(());
    };
    if tool == "help" {
        // A help.describe action carries the route it describes, not a route
        // owned by the help meta-tool. Validate the complete help invocation,
        // but keep the action compact instead of attaching the described
        // schema a second time.
        decode_public_call(tool, action["arguments"].clone())
            .map_err(|_| Error::InternalInvariant)?;
        return Ok(());
    }
    let Some(route) = action["arguments"]["route"].as_str() else {
        return Ok(());
    };
    let spec = route_for(tool, route).ok_or(Error::InternalInvariant)?;
    if planning_route(spec.route) {
        action["schema_help"] = schema_help_action(spec.tool, &json!({"route":spec.route}))?
            .ok_or(Error::InternalInvariant)?;
    } else {
        action["route_contract"] = describe_route(&spec);
    }
    Ok(())
}

fn tool_summary(tool: &str) -> &'static str {
    match tool {
        "get_state" => "Read bounded DB-only state for the current native session.",
        "query" => "Run one of nineteen named read-only routes.",
        "command" => "Run one of forty-one named logical state-transition routes.",
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

pub(crate) fn read_only_internal_call(name: &str) -> bool {
    matches!(name, "get_state" | "help" | "query")
        || route_for_internal(name).is_some_and(|route| route.tool == "query")
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

/// Resolve only a registered route's internal identity without interpreting caller params.
pub(crate) fn recognized_internal_name(name: &str, arguments: &Value) -> Option<&'static str> {
    if matches!(name, "query" | "command" | "execute") {
        route_for(name, arguments.get("route")?.as_str()?).map(|spec| spec.internal)
    } else {
        route_for_internal(name).map(|spec| spec.internal)
    }
}

/// Build the compact schema recovery call for a known routed invocation.
///
/// Public routed calls retain their requested route even when their params fail
/// decoding. Internal calls are resolved through the same route registry. An
/// unknown tool or route deliberately returns no action so callers can retain
/// the legacy state-reload fallback.
pub(crate) fn schema_help_action(name: &str, arguments: &Value) -> Result<Option<Value>> {
    let spec = if matches!(name, "query" | "command" | "execute") {
        arguments
            .get("route")
            .and_then(Value::as_str)
            .and_then(|route| route_for(name, route))
    } else {
        route_for_internal(name)
    };
    let Some(spec) = spec else {
        return Ok(None);
    };
    let arguments = json!({"mode":"describe","tool":spec.tool,"route":spec.route});
    parse_help(arguments.clone())?;
    Ok(Some(
        json!({"kind":"ready_call","tool":"help","arguments":arguments}),
    ))
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
    let mut properties = if tool == "get_state" {
        Some(std::collections::BTreeSet::new())
    } else {
        route_for_internal(internal).and_then(|spec| allowed_properties(&spec.schema))
    };
    // Legacy phase completion actions may carry backend-issued dependency
    // receipts even though the current v0.7 public schema deliberately omits
    // these caller-forbidden fields.
    if internal == "slice_pipeline_phase_complete"
        && let Some(properties) = properties.as_mut()
    {
        properties.extend(["consumed_outputs".to_owned(), "consumed_inputs".to_owned()]);
    }
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

#[cfg(test)]
pub(crate) fn planning_route(route: &str) -> bool {
    route.starts_with("program.") || route.starts_with("scope.candidates.") || route == "scope.open"
}
