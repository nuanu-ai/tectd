use crate::program_output::{begin_action, input_action, within_capacity};
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
    for native in &state.native_planning {
        calls.extend(native_actions(native)?);
    }
    for candidate in &state.candidate_sets {
        calls.push(crate::api::ready_action(
            "candidate_context",
            json!({"candidate_set_id":candidate.id,"view":"overview","limit":25}),
        )?);
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

fn native_actions(summary: &NativePlanningSummary) -> Result<Vec<Value>> {
    let mut calls = Vec::new();
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
    result["next_action"] = calls.first().map_or(Value::Null, |call| {
        call["arguments"]
            .get("route")
            .cloned()
            .unwrap_or_else(|| call["tool"].clone())
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{NativeSliceSummary, NativeWorkCandidateSummary, SliceState};

    fn summary() -> NativePlanningSummary {
        NativePlanningSummary {
            scope_id: Uuid::new_v4(),
            scope_revision: 1,
            candidate_set_id: Uuid::new_v4(),
            candidate_set_revision: 3,
            candidate_set_status: SliceCandidateSetStatus::Ready,
            snapshot_id: Uuid::new_v4(),
            stale: false,
            eligible_work: vec![NativeWorkCandidateSummary {
                candidate_id: Uuid::new_v4(),
                candidate_revision: 1,
            }],
            slices_needing_result: Vec::new(),
        }
    }

    #[test]
    fn native_state_routes_stale_ready_and_open_slice_without_stub_execution() {
        let ready = native_actions(&summary()).unwrap();
        assert_eq!(ready[0]["arguments"]["route"], "slice.open");
        assert!(ready.iter().all(|action| {
            !matches!(
                action["arguments"]["route"].as_str(),
                Some("slice.start" | "slice.execute")
            )
        }));

        let mut stale = summary();
        stale.stale = true;
        let stale_actions = native_actions(&stale).unwrap();
        assert_eq!(
            stale_actions[0]["arguments"]["route"],
            "slice.candidates.refresh"
        );

        let mut awaiting = summary();
        awaiting.eligible_work.clear();
        awaiting.slices_needing_result.push(NativeSliceSummary {
            slice_id: Uuid::new_v4(),
            slice_revision: 1,
            state: SliceState::Open,
        });
        let result_actions = native_actions(&awaiting).unwrap();
        assert_eq!(
            result_actions[0]["arguments"]["route"],
            "slice.result.record"
        );

        awaiting.slices_needing_result[0].state = SliceState::Blocked;
        let blocked_actions = native_actions(&awaiting).unwrap();
        assert!(
            blocked_actions
                .iter()
                .all(|action| action["arguments"]["route"] != "slice.result.record")
        );
        assert_eq!(
            blocked_actions[0]["arguments"]["route"],
            "slice.candidates.context"
        );
    }
}
