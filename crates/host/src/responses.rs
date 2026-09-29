mod intros;

pub(crate) use intros::error_intro;
use intros::intro;
use serde_json::{Value, json};
use tect_domain::Error;

const RESPONSE_RULES: &str = include_str!("../response-rules.txt");

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

pub(crate) fn success(data: Value) -> Value {
    content(intro(&data), data, false)
}

fn content(intro: &'static str, data: Value, is_error: bool) -> Value {
    json!({
        "content":[{"type":"text","text":intro},
            {"type":"text","text":serde_json::to_string(&data).expect("JSON value")},
            {"type":"text","text":RESPONSE_RULES}],
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
    match error.pipeline_source() {
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
        Error::StorageUnavailable | Error::TransportUnavailable | Error::OperationTimeout => {
            if let Some((name, arguments)) = call {
                if matches!(error.pipeline_source(), Error::OperationTimeout)
                    && let Some(id) = arguments
                        .pointer("/params/change_id")
                        .or_else(|| arguments.get("change_id"))
                        .and_then(Value::as_str)
                        .filter(|id| uuid::Uuid::parse_str(id).is_ok())
                {
                    actions.push(action(
                        "knowledge_lifecycle",
                        json!({"change_id":id,"view":"current"}),
                    )?);
                } else if name == "save_program" || name == "save_setup" {
                    actions.push(match reload {
                        Some(reload) => reload,
                        None => action("get_state", json!({}))?,
                    });
                } else if matches!(name, "query" | "command" | "execute") {
                    actions.push(json!({"kind":"ready_call","tool":name,"arguments":arguments}));
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
        Error::InvalidArguments | Error::InvalidArgumentsDetail(_) => {
            let schema_action = match call {
                Some((name, arguments)) => crate::api::schema_help_action(name, arguments)?,
                None => None,
            };
            actions.push(schema_action.unwrap_or(action("get_state", json!({}))?));
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
    if let Some(refusal) = error.refusal() {
        error_data["refusal"] =
            serde_json::to_value(refusal).map_err(|_| Error::InternalInvariant)?;
    }
    if let Some(diagnostic) = error.pipeline_artifact_diagnostic() {
        error_data["details"] =
            serde_json::to_value(diagnostic).map_err(|_| Error::InternalInvariant)?;
    }
    if let Some(diagnostic) = error.argument_diagnostic() {
        error_data["details"] =
            serde_json::to_value(diagnostic).map_err(|_| Error::InternalInvariant)?;
    }
    if let Some((name, arguments)) = call
        && matches!(name, "query" | "command" | "execute")
        && let Some(route) = arguments.get("route").and_then(Value::as_str)
        && let Some(contract) = crate::api::route_contract(name, route)
    {
        error_data["tool"] = json!(name);
        error_data["route"] = json!(route);
        error_data["route_contract"] = contract;
    }
    let data = with_actions(json!({"error":error_data}), actions, recommended);
    Ok(content(failure_intro(&error, call), data, true))
}

fn failure_intro(error: &Error, call: Option<(&str, &Value)>) -> &'static str {
    if !matches!(error.pipeline_source(), Error::OperationTimeout) {
        return error_intro(error);
    }
    match call {
        Some(("get_state" | "help" | "query", _)) => {
            "The daemon reached its operation deadline while reading. Retry the read if needed."
        }
        Some((_, arguments))
            if arguments
                .pointer("/params/request_id")
                .or_else(|| arguments.get("request_id"))
                .and_then(Value::as_str)
                .is_some() =>
        {
            "The daemon reached its operation deadline. Read saved state when available; if the result is still absent, retry only the exact same request ID and payload. Do not create a replacement record."
        }
        _ => error_intro(error),
    }
}

fn internal_failure() -> Value {
    let data = with_actions(
        json!({"error":{"code":Error::InternalInvariant.code()}}),
        Vec::new(),
        None,
    );
    content(error_intro(&Error::InternalInvariant), data, true)
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
            Error::OperationTimeout,
            Error::InvalidArguments,
            Error::Unauthorized,
        ] {
            assert!(error_intro(&error).len() <= 2000);
        }
        let large = "narrative".repeat(10_000);
        let response = success(json!({"large":large}));
        assert!(response.get("structuredContent").is_none());
        assert_eq!(response["content"].as_array().unwrap().len(), 3);
        let parsed: Value =
            serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["large"], large);
        assert_eq!(response["content"][2]["type"], "text");
        assert_eq!(response["content"][2]["text"], RESPONSE_RULES);
        assert_eq!(
            encoded_len(&json!({"large":large})).unwrap(),
            serde_json::to_vec(&response).unwrap().len()
        );
        let mut without_rules = response.clone();
        without_rules["content"].as_array_mut().unwrap().pop();
        assert!(
            encoded_len(&json!({"large":large})).unwrap()
                > serde_json::to_vec(&without_rules).unwrap().len()
        );
    }

    #[test]
    fn normal_and_fallback_failures_keep_json_and_rules_in_place() {
        for response in [failure(Error::Unauthorized, None), internal_failure()] {
            assert_eq!(response["isError"], true);
            assert_eq!(response["content"].as_array().unwrap().len(), 3);
            let payload: Value =
                serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
            assert!(payload["error"]["code"].is_string());
            assert_eq!(response["content"][2]["type"], "text");
            assert_eq!(response["content"][2]["text"], RESPONSE_RULES);
        }
    }

    #[test]
    fn operation_deadline_reads_existing_change_before_same_id_retry() {
        let change_id = uuid::Uuid::new_v4();
        let request_id = uuid::Uuid::new_v4();
        let arguments = json!({"route":"knowledge.change_phase_complete","params":{
            "change_id":change_id,"request_id":request_id,"phase_id":"kc-impact-plan"
        }});
        let response = failure(Error::OperationTimeout, Some(("command", &arguments)));
        let body: Value =
            serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["error"]["code"], "operation_timeout");
        assert_eq!(
            body["actions"][0]["arguments"]["route"],
            "knowledge.lifecycle"
        );
        assert_eq!(
            body["actions"][0]["arguments"]["params"]["change_id"],
            json!(change_id)
        );
        assert!(
            response["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("same request ID")
        );

        let begin = json!({"route":"knowledge.change_begin","params":{"request_id":request_id}});
        let response = failure(Error::OperationTimeout, Some(("command", &begin)));
        let body: Value =
            serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["actions"][0]["arguments"], begin);
        assert_eq!(body["recommended_action"], 0);

        let read = failure(Error::OperationTimeout, Some(("get_state", &json!({}))));
        assert!(
            read["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("Retry the read")
        );
        assert!(
            !read["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("request ID")
        );
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

    #[test]
    fn typed_refusal_is_additive_to_legacy_error_code() {
        let value = failure(Error::StaleRevision, None);
        let body: Value =
            serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["error"]["code"], "stale_revision");
        assert_eq!(body["error"]["refusal"]["code"], "STALE_REVISION");
        assert_eq!(body["error"]["refusal"]["next_action"], "refresh");
        assert_eq!(body["error"]["refusal"]["required"], "revision");
    }

    fn failure_body(error: Error, name: &str, arguments: &Value) -> Value {
        let value = failure(error, Some((name, arguments)));
        serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn invalid_known_routes_recommend_executable_route_specific_schema_help() {
        for (tool, route) in [
            ("command", "scope.candidates.begin"),
            ("query", "program.get"),
            ("execute", "setup.apply"),
        ] {
            let arguments = json!({"route":route,"params":{"invalid":true}});
            let body = failure_body(Error::InvalidArguments, tool, &arguments);
            assert_eq!(body["error"]["code"], "invalid_arguments");
            assert_eq!(body["error"]["refusal"]["code"], "INPUT_SCHEMA_INVALID");
            assert_eq!(
                body["error"]["refusal"]["next_action"],
                "correct_input_and_retry"
            );
            assert_eq!(body["error"]["refusal"]["required"], "schema_valid_input");
            assert_eq!(body["error"]["tool"], tool);
            assert_eq!(body["error"]["route"], route);
            assert!(body["error"]["route_contract"]["params_schema"].is_object());
            assert_eq!(body["recommended_action"], 0);
            let action = &body["actions"][0];
            assert_eq!(action["kind"], "ready_call");
            assert_eq!(action["tool"], "help");
            assert_eq!(
                action["arguments"],
                json!({"mode":"describe","tool":tool,"route":route})
            );
            assert_eq!(action["route_contract"]["route"], route);

            let request = crate::api::parse_help(action["arguments"].clone()).unwrap();
            let described = crate::api::help(request).unwrap();
            assert_eq!(described["kind"], "route");
            assert_eq!(described["tool"], tool);
            assert_eq!(described["route"], route);
            assert!(described["params_schema"].is_object());

            // One refusal carries the executable schema and example directly.
            assert!(action["route_contract"]["example"].is_object());
        }
    }

    #[test]
    fn schema_refusal_reports_pointer_and_full_nested_candidate_contract() {
        let arguments = json!({"route":"scope.candidates.save","params":{"kind":"draft"}});
        let body = failure_body(
            Error::invalid_arguments_from("missing field `candidate_set_id`"),
            "command",
            &arguments,
        );
        assert_eq!(body["error"]["refusal"]["code"], "INPUT_SCHEMA_INVALID");
        assert_eq!(
            body["error"]["details"]["violation_code"],
            "required_field_missing"
        );
        assert_eq!(
            body["error"]["details"]["pointer"],
            "/params/candidate_set_id"
        );
        let schema = &body["error"]["route_contract"]["params_schema"];
        assert!(schema["oneOf"][0]["properties"]["draft"]["properties"]["candidates"]["items"]
            ["properties"]["coverage_goals"]
            .is_object());
        assert_eq!(
            body["error"]["route_contract"]["example"]["arguments"]["route"],
            "scope.candidates.save"
        );
    }

    #[test]
    fn invalid_unknown_or_unrouted_calls_keep_state_fallback() {
        for (name, arguments) in [
            ("command", json!({"route":"unknown","params":{}})),
            ("unknown", json!({})),
        ] {
            let body = failure_body(Error::InvalidArguments, name, &arguments);
            assert_eq!(body["recommended_action"], 0);
            assert_eq!(
                body["actions"][0],
                json!({"kind":"ready_call","tool":"get_state","arguments":{}})
            );
        }
    }
}
