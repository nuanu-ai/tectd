use crate::program_output::{input_action, within_capacity};
use crate::responses::{action, with_actions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_domain::{
    Error, FileObservation, NativePlanningSummary, ProgramSummary, Result, SetupContext,
    SetupDiscovery, SetupFileStatus, SliceCandidateSetStatus, WorkspaceState,
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
                "format":"Absolute physical launch directory already supplied in the current task environment. The agent supplies this known context; do not ask the human to select a folder or use the MCP package/source/worktree directory."}]}),
        )
    }
}

fn actions(
    state: &WorkspaceState,
    programs: &[ProgramSummary],
    next: &Option<String>,
    file: Option<&FileObservation>,
    fallback: bool,
    params: &crate::workspace_state::Params,
) -> Result<Vec<Value>> {
    // Preserve trusted Application workflow-suggestion suppression. This is not
    // Host authorization, role inference, or data redaction.
    if state.next_action.is_none() {
        return Ok(Vec::new());
    }
    if state.workspace.is_none() {
        return Ok(vec![action("open_workspace", json!({}))?]);
    }
    let context = state.setup_context.as_ref();
    let mut calls = Vec::new();
    for native in &state.native_planning {
        calls.extend(native_actions(native)?);
    }
    calls.extend(candidate_actions(&state.candidate_sets)?);
    for (view, after) in [
        (
            crate::workspace_state::View::CandidateSets,
            state.candidate_sets_next_after.as_ref(),
        ),
        (
            crate::workspace_state::View::NativePlanning,
            state.native_planning_next_after.as_ref(),
        ),
    ] {
        if let Some(after) = after {
            let mut page = params.clone();
            page.view = view;
            page.after = Some(after.encode());
            page.limit = Some(25);
            calls.push(crate::workspace_state::action(&page)?);
        }
    }
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
            json!({"request_id":seed_request_id(params.action_seed,"setup-begin")}),
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
    calls.push(input_action(
        "begin_program",
        json!({"request_id":seed_request_id(params.action_seed,"program-begin")}),
    )?);
    Ok(calls)
}

pub(crate) fn candidate_actions(
    candidates: &[tect_domain::CandidateSetSummary],
) -> Result<Vec<Value>> {
    candidates
        .iter()
        .map(|candidate| {
            crate::api::ready_action(
                "candidate_context",
                json!({"candidate_set_id":candidate.id,"view":"overview","limit":25}),
            )
        })
        .collect()
}

fn seed_request_id(seed: Uuid, label: &str) -> Uuid {
    native_request_id(seed, 0, label)
}

pub(crate) fn native_actions(summary: &NativePlanningSummary) -> Result<Vec<Value>> {
    let mut calls = Vec::new();
    for run in &summary.pipeline_runs {
        calls.push(crate::api::ready_action(
            "slice_pipeline_context",
            json!({"run_id":run.run_id}),
        )?);
    }
    if summary.stale {
        calls.push(crate::api::ready_action(
            "refresh_slice_candidate_set",
            json!({"scope_id":summary.scope_id,"candidate_set_id":summary.candidate_set_id,
                "revision":summary.candidate_set_revision,
                "request_id":native_request_id(summary.candidate_set_id, summary.candidate_set_revision, "refresh")}),
        )?);
    } else {
        for slice in &summary.slices_needing_result {
            if slice.state != tect_domain::SliceState::Open {
                continue;
            }
            calls.push(crate::api::needs_action(
                "needs_input",
                "slice_result_record",
                json!({"request_id":native_request_id(slice.slice_id, slice.slice_revision, "result"),
                    "scope_id":summary.scope_id,"slice_id":slice.slice_id,
                    "slice_revision":slice.slice_revision}),
                "input",
                json!({"fields":[
                    {"path":"arguments.params.outcome","format":"completed or blocked"},
                    {"path":"arguments.params.summary","format":"Exact externally reported bounded outcome summary"},
                    {"path":"arguments.params.evidence","format":"One or more direct evidence records with kind, reference, and observation"},
                    {"path":"arguments.params.scope_impact","format":"How this observed result affects the remaining Scope plan"},
                    {"path":"arguments.params.remaining_work","format":"Remaining work after this result"}
                ]}),
            )?);
        }
        if matches!(summary.candidate_set_status, SliceCandidateSetStatus::Ready) {
            for work in &summary.eligible_work {
                calls.push(crate::api::ready_action(
                    "slice_open",
                    json!({"request_id":native_request_id(work.candidate_id, work.candidate_revision, "open"),
                        "scope_id":summary.scope_id,"scope_revision":summary.scope_revision,
                        "candidate_set_id":summary.candidate_set_id,
                        "candidate_set_revision":summary.candidate_set_revision,
                        "candidate_snapshot_id":summary.snapshot_id,
                        "candidate_id":work.candidate_id,
                        "candidate_revision":work.candidate_revision}),
                )?);
            }
        }
    }
    calls.push(crate::api::ready_action(
        "slice_candidate_context",
        json!({"scope_id":summary.scope_id,"view":"overview","limit":25}),
    )?);
    Ok(calls)
}

fn native_request_id(id: Uuid, revision: i64, operation: &str) -> Uuid {
    let digest =
        Sha256::digest(format!("tectd-native-state:{id}:{revision}:{operation}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn value(
    state: &WorkspaceState,
    file: Option<&FileObservation>,
    fallback: bool,
    rules: bool,
    params: &crate::workspace_state::Params,
) -> Result<Value> {
    let calls = actions(
        state,
        &state.programs,
        &state.next_after,
        file,
        fallback,
        params,
    )?;
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
    if rules {
        result["response_rules"] = json!(crate::responses::RESPONSE_RULES);
    }
    result["programs_delivery"] = json!(if fallback {
        "use_list_programs"
    } else {
        "listed"
    });
    result["next_action"] = calls.first().map_or(Value::Null, |call| {
        call["arguments"]
            .get("route")
            .cloned()
            .unwrap_or_else(|| call["tool"].clone())
    });
    Ok(with_actions(result, calls, Some(0)))
}

pub(crate) fn logical(
    state: &WorkspaceState,
    file: Option<&FileObservation>,
    rules: bool,
    params: &crate::workspace_state::Params,
) -> Result<Value> {
    value(state, file, false, rules, params)
}

pub(crate) fn workspace(state: WorkspaceState, capacity: usize) -> Result<Value> {
    encode(
        state,
        None,
        capacity,
        false,
        crate::workspace_state::params(
            crate::workspace_state::Origin::State,
            Uuid::new_v4(),
            None,
            None,
        ),
    )
}
pub(crate) fn opened(state: WorkspaceState, capacity: usize) -> Result<Value> {
    encode(
        state,
        None,
        capacity,
        true,
        crate::workspace_state::params(
            crate::workspace_state::Origin::Opened,
            Uuid::new_v4(),
            None,
            None,
        ),
    )
}
pub(crate) fn discovery(
    discovery: SetupDiscovery,
    capacity: usize,
    task_directory: Option<String>,
) -> Result<Value> {
    encode(
        discovery.state,
        Some(discovery.file),
        capacity,
        false,
        crate::workspace_state::params(
            crate::workspace_state::Origin::Discovery,
            Uuid::new_v4(),
            task_directory,
            Some(capacity),
        ),
    )
}

fn deferred(
    original: Value,
    params: &crate::workspace_state::Params,
    capacity: usize,
) -> Result<Value> {
    let wrapper = json!({"state":original});
    let bytes = serde_json::to_vec(&wrapper).map_err(|_| Error::TransportUnavailable)?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let mut root = wrapper["state"].clone();
    let mut fields = vec![];
    root.as_object_mut()
        .ok_or(Error::InternalInvariant)?
        .retain(|key, _| {
            let retain = matches!(
                key.as_str(),
                "status" | "workspace" | "session" | "response_rules"
            );
            if !retain {
                fields.push(key.clone());
            }
            retain
        });
    root["state_delivery"] = json!({"kind":"deferred","representation_digest":digest,"total_bytes":bytes.len(),"deferred_fields":fields});
    root["next_action"] = json!("workspace_state");
    let mut args = serde_json::to_value(params).map_err(|_| Error::TransportUnavailable)?;
    args["representation_digest"] = json!(digest);
    args["limit_bytes"] = json!(4096);
    within_capacity(
        with_actions(
            root,
            vec![crate::api::ready_action("workspace_state", args)?],
            Some(0),
        ),
        capacity,
    )
}

fn encode(
    mut state: WorkspaceState,
    file: Option<FileObservation>,
    capacity: usize,
    rules: bool,
    params: crate::workspace_state::Params,
) -> Result<Value> {
    let capacity = capacity.min(crate::json_fragment::READ_BUDGET);
    let original = logical(&state, file.as_ref(), rules, &params)?;
    if state.programs.is_empty() {
        return match within_capacity(original.clone(), capacity) {
            Ok(value) => Ok(value),
            Err(Error::RequestTooLarge) => deferred(original, &params, capacity),
            Err(error) => Err(error),
        };
    }
    let mut programs = std::mem::take(&mut state.programs);
    let original_next = state.next_after.clone();
    state.programs = vec![programs[0].clone()];
    state.next_after = next(&state.programs, programs.len() > 1, &original_next);
    let count = crate::program_output::paging::fitting_prefix(
        &programs,
        &value(&state, file.as_ref(), false, rules, &params)?,
        "programs",
        "next_after",
        capacity,
        |prefix, more| {
            let next = next(prefix, more, &original_next);
            Ok((
                actions(&state, prefix, &next, file.as_ref(), false, &params)?,
                json!(next),
            ))
        },
    );
    let fitted = match count {
        Ok(count) => {
            state.next_after = next(&programs[..count], count < programs.len(), &original_next);
            programs.truncate(count);
            state.programs = programs;
            within_capacity(
                value(&state, file.as_ref(), false, rules, &params)?,
                capacity,
            )
        }
        Err(Error::RequestTooLarge) => {
            state.programs.clear();
            state.next_after = None;
            within_capacity(
                value(&state, file.as_ref(), true, rules, &params)?,
                capacity,
            )
        }
        Err(error) => Err(error),
    };
    match fitted {
        Ok(value) => Ok(value),
        Err(Error::RequestTooLarge) => deferred(original, &params, capacity),
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

#[cfg(test)]
mod tests;

#[cfg(test)]
mod delivery_tests;
