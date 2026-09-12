use crate::program_output::{begin_action, input_action, within_capacity};
use crate::responses::{action, with_actions};
use serde_json::{Value, json};
use tect_domain::{
    Error, FileObservation, ProgramSummary, Result, SetupContext, SetupDiscovery, SetupFileStatus,
    WorkspaceState,
};
use uuid::Uuid;

pub(crate) fn inspect_action(context: Option<&SetupContext>) -> Result<Value> {
    if let Some(context) = context {
        action(
            "inspect_setup",
            json!({"task_directory":context.task_directory}),
        )
    } else {
        crate::api::needs_action(
            "needs_context",
            "inspect_setup",
            json!({}),
            "context_input",
            json!({"fields":[{"path":"arguments.params.task_directory",
                "format":"Absolute physical launch directory already supplied in the current Codex task environment context. The agent supplies this known context; do not ask the human to select a folder or use the MCP package/source/worktree directory."}]}),
        )
    }
}

fn actions(
    state: &WorkspaceState,
    programs: &[ProgramSummary],
    next: &Option<String>,
    file: Option<&FileObservation>,
    fallback: bool,
) -> Result<Vec<Value>> {
    if state.workspace.is_none() {
        return Ok(vec![action("open_workspace", json!({}))?]);
    }
    let context = state.setup_context.as_ref();
    let mut calls = Vec::new();
    match file.map(|file| file.status) {
        Some(SetupFileStatus::Unavailable) => {}
        Some(SetupFileStatus::ContextUnknown) => {}
        _ if context.and_then(|context| context.setup.as_ref()).is_some() => {
            calls.push(crate::setup_output::reload(
                context.and_then(|c| c.setup.as_ref()).expect("checked").id,
            )?);
        }
        Some(SetupFileStatus::Missing) => calls.push(input_action(
            "begin_setup",
            json!({"request_id":Uuid::new_v4()}),
        )?),
        None => {}
        Some(SetupFileStatus::Existing) => {}
    }
    if fallback {
        calls.push(action("list_programs", json!({"limit":25}))?);
    } else {
        for program in programs {
            calls.push(action("get_program", json!({"program_id":program.id}))?);
        }
        if let Some(after) = next {
            calls.push(action("list_programs", json!({"after":after,"limit":25}))?);
        }
    }
    if file.is_none()
        || file.is_some_and(|file| {
            matches!(
                file.status,
                SetupFileStatus::Unavailable | SetupFileStatus::ContextUnknown
            )
        })
    {
        calls.push(inspect_action(context)?);
    }
    calls.push(begin_action()?);
    Ok(calls)
}

fn value(state: &WorkspaceState, file: Option<&FileObservation>, fallback: bool) -> Result<Value> {
    let calls = actions(state, &state.programs, &state.next_after, file, fallback)?;
    let mut result = json!(state);
    result["file"] = file.map_or_else(
        || {
            json!({"status":"context_unknown",
        "observed_now":false,"reason":"filesystem_not_inspected_by_this_call"})
        },
        |file| {
            let mut result = json!(file);
            result["observed_now"] = json!(file.status != SetupFileStatus::ContextUnknown);
            result
        },
    );
    result["programs_delivery"] = json!(if fallback {
        "use_list_programs"
    } else {
        "listed"
    });
    result["next_action"] = calls
        .first()
        .map_or(Value::Null, |call| call["tool"].clone());
    Ok(with_actions(result, calls, Some(0)))
}

pub(crate) fn workspace(state: WorkspaceState, capacity: usize) -> Result<Value> {
    encode(state, None, capacity)
}
pub(crate) fn discovery(discovery: SetupDiscovery, capacity: usize) -> Result<Value> {
    encode(discovery.state, Some(discovery.file), capacity)
}

fn encode(
    mut state: WorkspaceState,
    file: Option<FileObservation>,
    capacity: usize,
) -> Result<Value> {
    if state.programs.is_empty() {
        return within_capacity(value(&state, file.as_ref(), false)?, capacity);
    }
    let mut programs = std::mem::take(&mut state.programs);
    let original_next = state.next_after.clone();
    state.programs = vec![programs[0].clone()];
    state.next_after = next(&state.programs, programs.len() > 1, &original_next);
    let count = crate::program_output::paging::fitting_prefix(
        &programs,
        &value(&state, file.as_ref(), false)?,
        "programs",
        "next_after",
        capacity,
        |prefix, more| {
            let next = next(prefix, more, &original_next);
            Ok((
                actions(&state, prefix, &next, file.as_ref(), false)?,
                json!(next),
            ))
        },
    );
    match count {
        Ok(count) => {
            state.next_after = next(&programs[..count], count < programs.len(), &original_next);
            programs.truncate(count);
            state.programs = programs;
            value(&state, file.as_ref(), false)
        }
        Err(Error::RequestTooLarge) => {
            // An old maximum-sized name may predate setup context overhead. Its full value
            // remains available through the existing unchanged standalone Program list.
            state.programs.clear();
            state.next_after = None;
            within_capacity(value(&state, file.as_ref(), true)?, capacity)
        }
        Err(error) => Err(error),
    }
}

fn next(programs: &[ProgramSummary], more: bool, original: &Option<String>) -> Option<String> {
    if more {
        programs.last().map(|program| program.cursor().encode())
    } else {
        original.clone()
    }
}
