use serde::Deserialize;
use serde_json::{Value, json};
use tect_domain::{Error, MAX_SOURCE_PATH_BYTES, MAX_WORKTREES, Result};
use uuid::Uuid;

pub(crate) enum Invocation {
    Program(crate::program_tools::ProgramInvocation),
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
        _ => crate::program_tools::parse(name, arguments).map(Invocation::Program),
    }
}

fn empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(serde_json::Map::is_empty)
}

pub(crate) fn definitions() -> Value {
    let mut definitions = json!({
        "tools": [
            {
                "name": "open_workspace",
                "description": "Create or recover this native session's logical workspace.",
                "inputSchema": object_schema(json!({}), json!([])),
                "annotations": annotations(false)
            },
            {
                "name": "get_state",
                "description": "Read this native session's bounded workspace state.",
                "inputSchema": object_schema(json!({}), json!([])),
                "annotations": annotations(true)
            },
            {
                "name": "register_source",
                "description": "Register a Git worktree for this logical workspace and host.",
                "inputSchema": object_schema(
                    json!({"path": {"type": "string", "minLength": 1, "maxLength": MAX_SOURCE_PATH_BYTES}}),
                    json!(["path"])
                ),
                "annotations": annotations(false)
            },
            {
                "name": "select_worktrees",
                "description": "Replace this native session's selected worktree set.",
                "inputSchema": object_schema(
                    json!({"worktree_ids": {
                        "type": "array", "items": {"type": "string", "format": "uuid"},
                        "maxItems": MAX_WORKTREES, "uniqueItems": true
                    }}),
                    json!(["worktree_ids"])
                ),
                "annotations": annotations(false)
            },
            {
                "name": "list_sources",
                "description": "List a bounded page of registered worktrees for this workspace and host.",
                "inputSchema": object_schema(
                    json!({
                        "after": {"type": "string", "format": "uuid"},
                        "limit": {"type": "integer", "minimum": 1, "maximum": MAX_WORKTREES}
                    }),
                    json!(["limit"])
                ),
                "annotations": annotations(true)
            }
        ]
    });
    definitions["tools"]
        .as_array_mut()
        .expect("tool array")
        .extend(crate::program_tools::definitions());
    definitions
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
    fn schemas_expose_eleven_bounded_tools() {
        let definitions = definitions();
        let tools = definitions["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 11);
        assert!(
            tools
                .iter()
                .all(|tool| tool["inputSchema"]["additionalProperties"] == false)
        );
        assert_eq!(
            tools[3]["inputSchema"]["properties"]["worktree_ids"]["maxItems"],
            100
        );
        assert_eq!(
            tools[4]["inputSchema"]["properties"]["limit"]["maximum"],
            100
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
