mod failure_budget;
mod intros;

pub(crate) use intros::error_intro;
use intros::{failure_intro, intro};
use serde_json::{Value, json};
use tect_domain::Error;

pub(crate) const RESPONSE_RULES: &str = include_str!("../response-rules.txt");
pub(crate) const RESPONSE_FOOTER: &str = "Follow the rules from workspace.open or help {\"text\":\"response-rules\"}. Required checks, approvals and authority still apply. Dependencies alone grant no permission or automatic resumption. Claim monitoring or continuation only when real.";

#[cfg(test)]
#[path = "responses/verifier_tests.rs"]
mod verifier_tests;

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
            {"type":"text","text":RESPONSE_FOOTER}],
        "isError":is_error
    })
}

pub(crate) fn encoded_len(data: &Value) -> tect_domain::Result<usize> {
    serde_json::to_vec(&success(data.clone()))
        .map(|bytes| bytes.len())
        .map_err(|_| Error::TransportUnavailable)
}

#[derive(Debug)]
pub(crate) enum FailureBuildError {
    EnvelopeCannotFit,
    Construction {
        #[cfg(test)]
        error: Error,
    },
}
impl FailureBuildError {
    pub(crate) fn construction(_error: Error) -> Self {
        Self::Construction {
            #[cfg(test)]
            error: _error,
        }
    }
}

#[cfg(test)]
pub(crate) fn failure(error: Error, call: Option<(&str, &Value)>) -> Value {
    failure_with_state(error, call, None)
}
#[cfg(test)]
pub(crate) fn failure_with_state(
    error: Error,
    call: Option<(&str, &Value)>,
    state: Option<&Value>,
) -> Value {
    match failure_bounded(error, call, state, 8192) {
        Ok(value) => value,
        Err(FailureBuildError::Construction { .. }) => internal_failure(),
        Err(FailureBuildError::EnvelopeCannotFit) => crate::mcp::envelope_too_large(Value::Null),
    }
}
pub(crate) fn failure_bounded(
    error: Error,
    call: Option<(&str, &Value)>,
    state: Option<&Value>,
    capacity: usize,
) -> std::result::Result<Value, FailureBuildError> {
    let data = failure_data(error.clone(), call, state).map_err(FailureBuildError::construction)?;
    failure_budget::fit(&error, call, data, capacity)
}
#[cfg(test)]
fn try_failure_with_state(
    error: Error,
    call: Option<(&str, &Value)>,
    state: Option<&Value>,
) -> tect_domain::Result<Value> {
    failure_bounded(error, call, state, 8192).map_err(|error| match error {
        FailureBuildError::EnvelopeCannotFit => Error::RequestTooLarge,
        FailureBuildError::Construction { error } => error,
    })
}
fn failure_data(
    error: Error,
    call: Option<(&str, &Value)>,
    state: Option<&Value>,
) -> tect_domain::Result<Value> {
    let denied = access_denial(&error);
    if denied {
        let mut data = json!({"code":error.code()});
        if let Some(refusal) = error.refusal() {
            data["refusal"] = json!({"code":refusal.code});
        }
        return Ok(with_actions(json!({"error":data}), Vec::new(), None));
    }
    let mut actions = Vec::new();
    let reload = reload_action(call)?;
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
            actions.push(match reload.clone() {
                Some(reload) => reload,
                None => action("get_state", json!({}))?,
            });
        }
        Error::StorageUnavailable | Error::TransportUnavailable | Error::OperationTimeout => {
            if let Some((name, arguments)) = call {
                if matches!(error.pipeline_source(), Error::OperationTimeout)
                    && valid_identity(call, "change_id").is_some()
                {
                    actions.push(action("knowledge_lifecycle",json!({"change_id":valid_identity(call,"change_id").unwrap(),"view":"current"}))?);
                } else if name == "save_program" || name == "save_setup" {
                    actions.push(reload.clone().unwrap_or(action("get_state", json!({}))?));
                } else if let Some(replay) = exact_replay(name, arguments) {
                    actions.push(replay);
                } else {
                    actions.push(reload.clone().unwrap_or(action("get_state", json!({}))?));
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
    if crate::pipeline_output::receipt_diff::is_receipt_failure(&error) {
        actions.clear();
        if let Some(arguments) = owner_arguments(call)
            && let Some(recovery) =
                crate::pipeline_output::receipt_diff::failure_recovery(&error, arguments)?
        {
            actions.push(recovery);
        }
    }
    if error.refusal().is_some_and(|refusal| {
        refusal.rule.as_deref() == Some("PIPELINE-SNAPSHOT-REFERENCE-MISSING")
    }) {
        actions.clear();
        if let Some(run_id) = valid_identity(call, "run_id") {
            actions.push(action(
                "slice_pipeline_context",
                json!({"run_id":run_id,"view":"current"}),
            )?);
        }
    }

    if error.refusal().is_some_and(|refusal| {
        refusal
            .rule
            .as_deref()
            .is_some_and(|rule| rule.starts_with("WP6-INSTRUCTION-PHASE-"))
    }) {
        actions.clear();
        if let Some(run_id) = valid_identity(call, "run_id") {
            actions.push(action(
                "slice_pipeline_context",
                json!({"run_id":run_id,"view":"current"}),
            )?);
        }
    }
    strip_failure_contracts(&mut actions);
    let recommended = (!actions.is_empty()).then_some(0);
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
    if matches!(
        error,
        Error::InvalidArguments | Error::InvalidArgumentsDetail(_)
    ) && let Some((name, arguments)) = call
        && let Some(help) = crate::api::schema_help_action(name, arguments)?
    {
        error_data["tool"] = help["arguments"]["tool"].clone();
        error_data["route"] = help["arguments"]["route"].clone();
        error_data["schema_help"] = help;
    }
    Ok(with_actions(
        json!({"error":error_data}),
        actions,
        recommended,
    ))
}

fn strip_failure_contracts(actions: &mut [Value]) {
    for action in actions {
        if let Some(action) = action.as_object_mut() {
            action.remove("route_contract");
            action.remove("next_action_contract");
        }
    }
}
fn access_denial(error: &Error) -> bool {
    matches!(
        error.pipeline_source(),
        Error::Unauthorized
            | Error::InvalidNativeSession
            | Error::InvalidWorkspaceKey
            | Error::SessionRevoked
            | Error::SessionWorkspaceMismatch
            | Error::Forbidden
            | Error::InvalidConfiguration
            | Error::TaskDirectoryMismatch
            | Error::SetupUnavailable
    )
}
fn owner_arguments<'a>(call: Option<(&str, &'a Value)>) -> Option<&'a Value> {
    let (name, args) = call?;
    if matches!(name, "query" | "command" | "execute") {
        crate::api::schema_help_action(name, args).ok().flatten()?;
        args.get("params").filter(|params| params.is_object())
    } else {
        if !matches!(name, "get_state" | "help")
            && crate::api::schema_help_action(name, args)
                .ok()
                .flatten()
                .is_none()
        {
            return None;
        }
        args.as_object()?;
        Some(args)
    }
}
fn valid_identity(call: Option<(&str, &Value)>, key: &str) -> Option<uuid::Uuid> {
    if !matches!(key, "setup_id" | "program_id" | "change_id" | "run_id") {
        return None;
    }
    owner_arguments(call)?
        .get(key)?
        .as_str()?
        .parse::<uuid::Uuid>()
        .ok()
        .filter(|id| !id.is_nil())
}
fn reload_action(call: Option<(&str, &Value)>) -> tect_domain::Result<Option<Value>> {
    for key in ["setup_id", "program_id", "change_id", "run_id"] {
        if let Some(id) = valid_identity(call, key) {
            return Ok(Some(match key {
                "setup_id" => action(
                    "get_setup",
                    json!({"setup_id":id,"after_input":0,"limit":25}),
                )?,
                "program_id" => action("get_program", json!({"program_id":id}))?,
                "change_id" => {
                    let internal = call
                        .and_then(|(name, args)| crate::api::recognized_internal_name(name, args));
                    if matches!(
                        internal,
                        Some(
                            "knowledge_lifecycle"
                                | "knowledge_change_begin"
                                | "knowledge_change_phase_complete"
                                | "knowledge_change_record_input"
                                | "knowledge_change_commit"
                                | "knowledge_change_settle_effects"
                        )
                    ) {
                        action(
                            "knowledge_lifecycle",
                            json!({"change_id":id,"view":"current"}),
                        )?
                    } else {
                        action("knowledge_change", json!({"change_id":id}))?
                    }
                }
                _ => action("slice_pipeline_context", json!({"run_id":id}))?,
            }));
        }
    }
    Ok(None)
}
fn exact_replay(name: &str, args: &Value) -> Option<Value> {
    if matches!(name, "query" | "command" | "execute" | "help" | "get_state") {
        crate::api::decode_public_call(name, args.clone()).ok()?;
        Some(json!({"kind":"ready_call","tool":name,"arguments":args}))
    } else {
        action(name, args.clone()).ok()
    }
}
fn bounded_recovery(
    error: &Error,
    call: Option<(&str, &Value)>,
) -> tect_domain::Result<Vec<Value>> {
    if access_denial(error) {
        return Ok(Vec::new());
    }
    if error.refusal().is_some_and(|r| {
        matches!(
            r.rule.as_deref(),
            Some(
                "WP6-SKILL-READ-01"
                    | "WP6-RESOURCE-READ-01"
                    | "PIPELINE-SNAPSHOT-REFERENCE-MISSING"
            )
        )
    }) {
        return failure_data(error.clone(), call, None)?
            .get("actions")
            .and_then(Value::as_array)
            .cloned()
            .ok_or(Error::InternalInvariant);
    }
    if let Some(reload) = reload_action(call)? {
        return Ok(vec![reload]);
    }
    if let Some((name, args)) = call
        && let Some(help) = crate::api::schema_help_action(name, args)?
    {
        return Ok(vec![help]);
    }
    Ok(vec![action("get_state", json!({}))?])
}

pub(crate) fn internal_failure_bounded(capacity: usize) -> Option<Value> {
    let value = internal_failure();
    (serde_json::to_vec(&value).ok()?.len() <= capacity.min(8192)).then_some(value)
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
mod tests;

#[cfg(test)]
mod engineering_path_tests;

#[cfg(test)]
mod failure_budget_tests;

#[cfg(test)]
mod p8a_guidance_tests;
