use serde::Deserialize;
use serde_json::{Value, json};
use tect_domain::{Error, Result};
use uuid::Uuid;

pub(crate) enum Invocation {
    Program(crate::program_tools::ProgramInvocation),
    Setup(crate::setup_tools::SetupInvocation),
    ScopeCandidate(crate::scope_candidate_tools::ScopeCandidateInvocation),
    Slice(crate::slice_tools::SliceInvocation),
    Pipeline(crate::pipeline_tools::PipelineInvocation),
    Help(crate::api::HelpRequest),
    OpenWorkspace,
    GetState,
    RegisterSource { path: String },
    SelectWorktrees { worktree_ids: Vec<Uuid> },
    ListSources { after: Option<Uuid>, limit: u32 },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegisterSourceArguments {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectWorktreesArguments {
    worktree_ids: Vec<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListSourcesArguments {
    #[serde(default)]
    after: Option<Uuid>,
    limit: u32,
}

pub(crate) fn parse_invocation(name: &str, arguments: Value) -> Result<Invocation> {
    match name {
        "open_workspace" if empty_object(&arguments) => Ok(Invocation::OpenWorkspace),
        "get_state" if empty_object(&arguments) => Ok(Invocation::GetState),
        "register_source" => {
            let arguments = serde_json::from_value::<RegisterSourceArguments>(arguments)
                .map_err(|_| Error::InvalidArguments)?;
            Ok(Invocation::RegisterSource {
                path: arguments.path,
            })
        }
        "select_worktrees" => {
            let arguments = serde_json::from_value::<SelectWorktreesArguments>(arguments)
                .map_err(|_| Error::InvalidArguments)?;
            Ok(Invocation::SelectWorktrees {
                worktree_ids: arguments.worktree_ids,
            })
        }
        "list_sources" => {
            if arguments.get("after").is_some_and(Value::is_null) {
                return Err(Error::InvalidArguments);
            }
            let arguments = serde_json::from_value::<ListSourcesArguments>(arguments)
                .map_err(|_| Error::InvalidArguments)?;
            Ok(Invocation::ListSources {
                after: arguments.after,
                limit: arguments.limit,
            })
        }
        "help" => crate::api::parse_help(arguments).map(Invocation::Help),
        _ if name.starts_with("slice_pipeline_") => {
            crate::pipeline_tools::parse(name, arguments).map(Invocation::Pipeline)
        }
        _ if matches!(
            name,
            "scope_context"
                | "slice_pipelines"
                | "slice_candidate_context"
                | "scope_open"
                | "save_slice_candidate_set"
                | "record_slice_candidate_input"
                | "refresh_slice_candidate_set"
                | "slice_open"
                | "slice_context"
                | "slice_result_record"
        ) =>
        {
            crate::slice_tools::parse(name, arguments).map(Invocation::Slice)
        }
        _ if name.contains("candidate") => {
            crate::scope_candidate_tools::parse(name, arguments).map(Invocation::ScopeCandidate)
        }
        _ if name.ends_with("_setup")
            || name == "record_setup_input"
            || name == "read_skill" && arguments["name"] == "tectd-setup" =>
        {
            crate::setup_tools::parse(name, arguments).map(Invocation::Setup)
        }
        _ => crate::program_tools::parse(name, arguments).map(Invocation::Program),
    }
}

fn empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(serde_json::Map::is_empty)
}

pub(crate) fn object_schema(properties: Value, required: Value) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

pub(crate) fn annotations(read_only: bool) -> Value {
    json!({
        "readOnlyHint": read_only,
        "idempotentHint": true,
        "destructiveHint": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schemas_expose_five_bounded_tools() {
        let definitions = crate::api::definitions();
        let tools = definitions["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 5);
        assert!(
            tools
                .iter()
                .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
        );
        assert_eq!(
            tools[2]["inputSchema"]["properties"]["route"]["enum"]
                .as_array()
                .unwrap()
                .len(),
            24
        );
    }

    #[test]
    fn argument_decoding_rejects_unknown_and_wrongly_typed_fields() {
        assert!(matches!(
            parse_invocation("register_source", json!({"path": "/repo"})),
            Ok(Invocation::RegisterSource { .. })
        ));
        for invalid in [
            json!({"path": "/repo", "workspace_key": "spoofed"}),
            json!({"path": "/repo", "native_session_id": Uuid::new_v4()}),
            json!({"path": 7}),
            json!([]),
        ] {
            assert!(matches!(
                parse_invocation("register_source", invalid),
                Err(Error::InvalidArguments)
            ));
        }
        assert!(matches!(
            parse_invocation("list_sources", json!({"after": null, "limit": 25})),
            Err(Error::InvalidArguments)
        ));
        assert!(matches!(
            parse_invocation("list_sources", json!({"limit": 1.5})),
            Err(Error::InvalidArguments)
        ));
    }
}
