use serde_json::{Value, json};
use tect_domain::Error;

pub(crate) const INTROS: [&str; 8] = [
    "This native session has no open workspace. Open it to continue.",
    "No Programs are registered in this workspace. Create one from your narrative.",
    "Continue an existing Program or start another using the actions below.",
    "Program is saved. Read its formation skill and required original input before composing the next revision.",
    "Program and pending question are saved. Ask that question, then record the original reply.",
    "Program is open. Present the whole saved PRD; record corrections when provided.",
    "Use this method with the current Program calls.",
    "Current data and available calls are below.",
];

pub(crate) fn action(tool: &str, arguments: Value) -> tect_domain::Result<Value> {
    crate::api::ready_action(tool, arguments)
}

pub(crate) fn with_actions(
    mut data: Value,
    actions: Vec<Value>,
    recommended: Option<usize>,
) -> Value {
    data["actions"] = json!(actions);
    data["recommended_action"] = json!(recommended);
    data
}

fn intro(data: &Value) -> &'static str {
    if data["name"] == "tectd-setup" && data.get("body").is_some() {
        return "Use this method with the current workspace setup calls.";
    }
    if let Some(step) = data["setup"]["current_step"].as_str() {
        return match step {
            "compose" => {
                "Setup is saved. Read its method and original input, then show and save the whole proposed AGENTS.md."
            }
            "waiting_input" => {
                "The setup draft and pending question are saved. Ask only that unresolved question, then record the complete original reply."
            }
            "ready_to_apply" => {
                "The complete setup draft is durable and ready. Show the whole file and use the exact apply call."
            }
            "complete"
                if data["file"]["observed_now"] == true
                    && (data["file"]["status"] == "missing"
                        || data["file"]["sha256"].is_string()
                            && data["file"]["sha256"] != data["setup"]["applied_sha256"]) =>
            {
                "Setup was applied previously, but the current file is missing or differs from the applied content. This conflict is preserved; no file was recreated or overwritten."
            }
            "complete" if data["file"]["observed_now"] == true => {
                "Setup is historically applied; the current file observation and whole saved content are below. Only matching current bytes verify that result now."
            }
            "complete" => {
                "Setup is historically applied. This call did not verify the file now; the whole saved content is below."
            }
            _ => INTROS[7],
        };
    }
    if data["name"] == "tectd-program" && data.get("body").is_some() {
        return INTROS[6];
    }
    if let Some(step) = data["program"]["current_step"].as_str() {
        return match step {
            "compose" => INTROS[3],
            "waiting_input" => INTROS[4],
            "ready" => INTROS[5],
            _ => INTROS[7],
        };
    }
    if data["status"] == "uninitialized" {
        return INTROS[0];
    }
    if data["programs_delivery"] == "use_list_programs" {
        return "The Program listing is available through the exact query route program.list call from the beginning. Full names did not fit beside this workspace context; the current file observation is included below.";
    }
    if data.get("file").is_some() {
        return match data["file"]["status"].as_str() {
            Some("missing") => {
                "AGENTS.md is currently absent in this task directory. Continue its saved setup or compose it from the existing company/work narrative. Programs remain available."
            }
            Some("existing") => {
                "AGENTS.md already exists and is preserved. Current setup context and available Programs are below."
            }
            Some("unavailable") => {
                "AGENTS.md could not be safely inspected with current access. This does not establish absence. Programs remain available."
            }
            _ if data["setup_context"]["setup"].is_object() => {
                "A saved setup is available. Use its exact query route setup.get call to restore the draft and original history and inspect the current file. Programs remain available."
            }
            _ => {
                "This is saved workspace state; the file's current presence is unknown. The exact inspection action accepts the known task launch directory. Programs remain available."
            }
        };
    }
    if let Some(programs) = data["programs"].as_array() {
        return if programs.is_empty() {
            INTROS[1]
        } else {
            INTROS[2]
        };
    }
    INTROS[7]
}

pub(crate) fn success(data: Value) -> Value {
    content(intro(&data), data, false)
}

fn content(intro: &'static str, data: Value, is_error: bool) -> Value {
    json!({
        "content":[{"type":"text","text":intro},
            {"type":"text","text":serde_json::to_string(&data).expect("JSON value")}],
        "isError":is_error
    })
}

pub(crate) fn encoded_len(data: &Value) -> tect_domain::Result<usize> {
    serde_json::to_vec(&success(data.clone()))
        .map(|bytes| bytes.len())
        .map_err(|_| Error::TransportUnavailable)
}

pub(crate) fn failure(error: Error, call: Option<(&str, &Value)>) -> Value {
    failure_with_state(error, call, None)
}

pub(crate) fn failure_with_state(
    error: Error,
    call: Option<(&str, &Value)>,
    state: Option<&Value>,
) -> Value {
    match try_failure_with_state(error, call, state) {
        Ok(value) => value,
        Err(_) => internal_failure(),
    }
}

fn try_failure_with_state(
    error: Error,
    call: Option<(&str, &Value)>,
    state: Option<&Value>,
) -> tect_domain::Result<Value> {
    let mut actions = Vec::new();
    let reload = if let Some((name, args)) = call {
        if let Some(id) = args.get("setup_id").filter(|id| id.is_string()) {
            Some(action(
                "get_setup",
                json!({"setup_id":id,"after_input":0,"limit":25}),
            )?)
        } else if let Some(id) = args.get("program_id").filter(|id| id.is_string()) {
            Some(action("get_program", json!({"program_id":id}))?)
        } else if let Some(id) = args.get("change_id").filter(|id| id.is_string()) {
            if matches!(
                name,
                "knowledge_lifecycle"
                    | "knowledge_change_begin"
                    | "knowledge_change_phase_complete"
                    | "knowledge_change_record_input"
                    | "knowledge_change_commit"
                    | "knowledge_change_settle_effects"
            ) {
                Some(action(
                    "knowledge_lifecycle",
                    json!({"change_id":id,"view":"current"}),
                )?)
            } else {
                Some(action("knowledge_change", json!({"change_id":id}))?)
            }
        } else if let Some(id) = args.get("run_id").filter(|id| id.is_string()) {
            Some(action("slice_pipeline_context", json!({"run_id":id}))?)
        } else {
            None
        }
    } else {
        None
    };
    match &error {
        Error::WorkspaceNotOpen => actions.push(action("open_workspace", json!({}))?),
        Error::StaleRevision
        | Error::StaleContext
        | Error::ContextChanged
        | Error::NeedsContext
        | Error::InputPending
        | Error::ProgramIncomplete
        | Error::SetupIncomplete
        | Error::SetupAlreadyApplied
        | Error::SetupFileConflict
        | Error::InputConflict
        | Error::RequestTooLarge
        | Error::CapacityExceeded => {
            actions.push(match reload {
                Some(reload) => reload,
                None => action("get_state", json!({}))?,
            });
        }
        Error::StorageUnavailable | Error::TransportUnavailable => {
            if let Some((name, arguments)) = call {
                if name == "save_program" || name == "save_setup" {
                    actions.push(match reload {
                        Some(reload) => reload,
                        None => action("get_state", json!({}))?,
                    });
                } else {
                    actions.push(action(name, arguments.clone())?);
                }
            }
        }
        Error::TaskDirectoryUnbound => actions.push(crate::workspace_output::inspect_action(None)?),
        Error::KnowledgeUnavailable => {
            actions.push(action("knowledge_context", json!({}))?);
        }
        Error::KnowledgeLifecycleRequired => {
            actions.push(action("knowledge_lifecycle", json!({}))?);
            if let Some(("slice_pipeline_begin", arguments)) = call {
                let mut params = json!({
                    "request_id":arguments["request_id"],
                    "intent":arguments["qualification_reason"],
                    "owner":{"kind":"promotion_slice","scope_id":arguments["scope_id"],
                        "slice_id":arguments["slice_id"],"slice_revision":arguments["slice_revision"]}
                });
                if let Some(mode) = arguments.get("delivery_mode") {
                    params["delivery_mode"] = mode.clone();
                }
                actions.push(crate::api::needs_action(
                    "needs_context",
                    "knowledge_change_begin",
                    params,
                    "context_input",
                    json!({"fields":[
                        {"path":"arguments.params.desired_outcome","format":"The exact bounded durable outcome requested for this Promotion Slice."},
                        {"path":"arguments.params.sources","format":"Exact saved source snapshots or authenticated producer output references."},
                        {"path":"arguments.params.operation_hints","format":"One to sixteen create, revise, revalidate, supersede, retract, or erase hints; backend assigns canonical identities."},
                        {"path":"arguments.params.completion","format":"Exact canonical, delivery, impact, search, and erasure completion contract."}
                    ]}),
                )?);
            }
        }
        Error::SetupExists if state.is_some() => {
            let context = state.and_then(|state| {
                serde_json::from_value::<tect_domain::SetupContext>(state["setup_context"].clone())
                    .ok()
            });
            actions.push(crate::workspace_output::inspect_action(context.as_ref())?);
        }
        Error::Unauthorized
        | Error::InvalidNativeSession
        | Error::SessionRevoked
        | Error::SessionWorkspaceMismatch
        | Error::Forbidden
        | Error::InvalidConfiguration
        | Error::TaskDirectoryMismatch
        | Error::SetupUnavailable => {}
        _ => actions.push(action("get_state", json!({}))?),
    }
    let recommended = (!actions.is_empty()).then_some(0);
    if state.is_some() {
        actions.push(action("list_programs", json!({"limit":25}))?);
        actions.push(crate::program_output::begin_action()?);
    }
    let recommended = recommended.or_else(|| (!actions.is_empty()).then_some(0));
    let mut error_data = json!({"code":error.code()});
    if let Some(diagnostic) = error.pipeline_artifact_diagnostic() {
        error_data["details"] =
            serde_json::to_value(diagnostic).map_err(|_| Error::InternalInvariant)?;
    }
    if let Some(diagnostic) = error.argument_diagnostic() {
        error_data["details"] =
            serde_json::to_value(diagnostic).map_err(|_| Error::InternalInvariant)?;
    }
    let data = with_actions(json!({"error":error_data}), actions, recommended);
    Ok(content(error_intro(&error), data, true))
}

fn internal_failure() -> Value {
    let data = with_actions(
        json!({"error":{"code":Error::InternalInvariant.code()}}),
        Vec::new(),
        None,
    );
    content(error_intro(&Error::InternalInvariant), data, true)
}

pub(crate) fn error_intro(error: &Error) -> &'static str {
    match error {
        Error::StaleRevision => {
            "A newer revision exists. Reload the saved record and merge before saving."
        }
        Error::StaleContext | Error::ContextChanged => {
            "The saved context changed. Reload its exact owner context and follow the supplied recovery action."
        }
        Error::NeedsContext => {
            "The current operation has unresolved context needs. Reload its exact owner context and follow the supplied recovery action."
        }
        Error::KnowledgeUnavailable => {
            "Durable knowledge is unavailable because its capability, database identity, or integrity check is not ready. Follow the supplied context or operator recovery action."
        }
        Error::KnowledgeLifecycleRequired => {
            "This durable change is owned by the Knowledge Change lifecycle. Continue with its exact owner and source-bound begin contract."
        }
        Error::CapacityExceeded => {
            "The complete required durable knowledge context exceeds the bounded transport capacity. No partial context was returned."
        }
        Error::InputPending => {
            "The input cursor is not current. Read the remaining original input before completing."
        }
        Error::ProgramIncomplete => {
            "Keep every required PRD field meaningful and resolve the pending question before completion."
        }
        Error::SetupIncomplete => {
            "The setup needs coherent content, every input incorporated and no pending question before execute route setup.apply can proceed."
        }
        Error::SetupFileConflict => {
            "The current AGENTS.md is missing after prior application, changed, or conflicts with the intended content. It was not overwritten or recreated. Reload the current observation."
        }
        Error::SetupAlreadyApplied => {
            "This initial setup is already applied and cannot be edited through setup. Reload its saved content and current file observation."
        }
        Error::TaskDirectoryUnbound => {
            "Supply the launch directory already known from this Codex task context through the exact inspection action; do not ask the human to choose a folder."
        }
        Error::SetupExists => {
            "A setup already exists for this task directory. Read workspace state and resume that same setup."
        }
        Error::InputConflict => {
            "This request identity already belongs to different original input. Reload; use a new request identity for a new input."
        }
        Error::WorkspaceNotOpen => "Open this native session's workspace before continuing.",
        Error::RequestTooLarge => {
            "The encoded request or response exceeds transport capacity. Reload current state before retrying."
        }
        Error::StorageUnavailable | Error::TransportUnavailable => {
            "The result is uncertain. Recover with the exact call below; do not create a replacement record."
        }
        Error::InvalidArguments => {
            "The arguments do not match the current tool schema. Read current state and use the live schema."
        }
        Error::InvalidArgumentsDetail(_) => {
            "The arguments do not match the current tool schema; details.reason names the field. Correct that field and retry."
        }
        Error::InvalidPipelineArtifact(_) => {
            "A pipeline artifact failed its phase contract. Correct every reported violation and retry the supplied phase action."
        }
        Error::InternalInvariant => {
            "TectD could not construct a valid next call. No follow-up action was emitted."
        }
        _ => {
            "The request cannot proceed with the current identity or access. No protected data is included."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_intro_has_a_budget_and_json_is_not_mirrored() {
        for intro in INTROS {
            assert!(intro.len() <= 2000);
        }
        for error in [
            Error::StaleRevision,
            Error::InputPending,
            Error::ProgramIncomplete,
            Error::InputConflict,
            Error::WorkspaceNotOpen,
            Error::RequestTooLarge,
            Error::StorageUnavailable,
            Error::TransportUnavailable,
            Error::InvalidArguments,
            Error::Unauthorized,
        ] {
            assert!(error_intro(&error).len() <= 2000);
        }
        let large = "narrative".repeat(10_000);
        let response = success(json!({"large":large}));
        assert!(response.get("structuredContent").is_none());
        assert_eq!(response["content"].as_array().unwrap().len(), 2);
        let parsed: Value =
            serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["large"], large);
    }

    #[test]
    fn promotion_refusal_returns_the_same_slice_owner_continuation() {
        let id = uuid::Uuid::new_v4();
        let arguments = json!({"request_id":id,"scope_id":id,"slice_id":id,
            "slice_revision":3,"qualification_reason":"Publish this bounded evidence.",
            "delivery_mode":"whole"});
        let value = try_failure_with_state(
            Error::KnowledgeLifecycleRequired,
            Some(("slice_pipeline_begin", &arguments)),
            None,
        )
        .unwrap();
        let value: Value =
            serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(
            value["actions"][0]["arguments"]["route"],
            "knowledge.lifecycle"
        );
        assert_eq!(
            value["actions"][1]["arguments"]["route"],
            "knowledge.change_begin"
        );
        assert_eq!(
            value["actions"][1]["arguments"]["params"]["owner"]["kind"],
            "promotion_slice"
        );
        assert_eq!(
            value["actions"][1]["arguments"]["params"]["owner"]["slice_revision"],
            3
        );
    }
}
