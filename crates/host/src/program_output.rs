use crate::frame::MAX_FRAME_BYTES;
use crate::responses::{action, encoded_len, with_actions};
use serde_json::{Value, json};
use sha2::Digest;
use tect_application::{ProgramGuidance, ProgramOutputGuard};
use tect_domain::{
    Error, MAX_SOURCE_PATH_BYTES, MAX_WORKTREES, Program, ProgramInput, ProgramList, ProgramPage,
    ProgramStep, ProgramSummary, Result, Session, Workspace, WorkspaceState, WorktreeSummary,
};
use uuid::Uuid;

pub(crate) const PROGRAM_SKILL: &str = include_str!("../../../skills/tectd-program/SKILL.md");
pub(crate) const PROGRAM_METHOD_ID: &str = "tectd-program";
pub(crate) const PROGRAM_METHOD_REVISION: &str = "1";

pub(crate) struct StaticProgramGuidance;

impl ProgramGuidance for StaticProgramGuidance {
    fn planning_method(&self) -> tect_domain::PlanningMethodSnapshot {
        let hash = sha2::Sha256::digest(PROGRAM_SKILL.as_bytes());
        tect_domain::PlanningMethodSnapshot {
            id: PROGRAM_METHOD_ID.into(),
            version: PROGRAM_METHOD_REVISION.into(),
            digest: hash.iter().map(|byte| format!("{byte:02x}")).collect(),
            body: PROGRAM_SKILL.into(),
            origin_refs: vec![format!(
                "skills/tectd-program/SKILL.md@{PROGRAM_METHOD_REVISION}"
            )],
        }
    }
}

pub(crate) mod paging;

fn skill_action() -> Result<Value> {
    crate::api::method_action("tectd-program")
}

pub(crate) fn input_action(tool: &str, arguments: Value) -> Result<Value> {
    crate::api::needs_action(
        "needs_input",
        tool,
        arguments,
        "input",
        json!({"fields":[{"path":"arguments.params.input","format":"Complete original user message, including request phrasing and context, as one nonblank string. Preserve exact text without extracting, trimming or paraphrasing."}]}),
    )
}

pub(crate) fn begin_action() -> Result<Value> {
    input_action("begin_program", json!({"request_id":Uuid::new_v4()}))
}

fn save_action(program: &Program, cursor: i64) -> Result<Value> {
    let mut params =
        json!({"program_id":program.id,"revision":program.revision,"input_cursor":cursor});
    if let Some(manifest) = program
        .planning_knowledge
        .as_ref()
        .and_then(|v| v.manifest.as_ref())
    {
        params["consumed_knowledge"] = json!({"manifest_id":manifest.id,"digest":manifest.digest,"workspace_generation":manifest.workspace_generation});
    }
    crate::api::needs_action(
        "needs_input",
        "save_program",
        params,
        "input",
        json!({"fields":[
            {"path":"arguments.params.name","format":"Optional string-or-null patch; omission preserves and null clears."},
            {"path":"arguments.params.intent","format":"Optional string-or-null patch; omission preserves and null clears."},
            {"path":"arguments.params.basis","format":"Optional string-or-null patch; omission preserves and null clears."},
            {"path":"arguments.params.boundaries","format":"Optional string-or-null patch; omission preserves and null clears."},
            {"path":"arguments.params.constraints","format":"Optional string-or-null patch; omission preserves and null clears."},
            {"path":"arguments.params.success","format":"Optional string-or-null patch; omission preserves and null clears."},
            {"path":"arguments.params.working_notes","format":"Optional string-or-null continuation patch."},
            {"path":"arguments.params.pending_question","format":"Optional string-or-null question patch."},
            {"path":"arguments.params.complete","format":"Optional boolean, default false; true requires six coherent nonblank fields, no pending question, and all original input incorporated."}
        ]}),
    )
}

fn program_actions(
    program: &Program,
    delivered: Option<i64>,
    next: Option<i64>,
) -> Result<Vec<Value>> {
    let mut actions = Vec::new();
    if program
        .planning_knowledge
        .as_ref()
        .is_some_and(|v| !v.stale_reasons.is_empty())
    {
        actions.push(action(
            "refresh_program_knowledge",
            json!({"program_id":program.id,"revision":program.revision,
                "input_cursor":program.input_cursor,"request_id":Uuid::new_v4()}),
        )?);
    }
    actions.push(skill_action()?);
    match program.current_step {
        ProgramStep::Compose => {
            if let Some(after_input) = next {
                actions.push(action(
                    "get_program",
                    json!({"program_id":program.id,"after_input":after_input,"limit":25}),
                )?);
            } else if delivered.is_none() && program.input_cursor < program.latest_input {
                actions.push(action("get_program", json!({"program_id":program.id}))?);
            }
            actions.push(save_action(
                program,
                delivered
                    .unwrap_or(program.input_cursor)
                    .max(program.input_cursor),
            )?);
        }
        ProgramStep::WaitingInput | ProgramStep::Ready => {
            let mut params = json!({"program_id":program.id,"request_id":Uuid::new_v4()});
            if let Some(manifest) = program
                .planning_knowledge
                .as_ref()
                .and_then(|v| v.manifest.as_ref())
            {
                params["task_context"] = serde_json::to_value(&manifest.task_context)
                    .map_err(|_| Error::TransportUnavailable)?;
            }
            actions.push(input_action("record_program_input", params)?);
        }
    }
    Ok(actions)
}

pub(crate) fn program(program: Program) -> Result<Value> {
    let actions = program_actions(&program, None, None)?;
    Ok(with_actions(json!({"program":program}), actions, Some(0)))
}

fn page_value(page: &ProgramPage) -> Result<Value> {
    let delivered = page
        .inputs
        .last()
        .map(|entry| entry.sequence)
        .or(Some(page.program.input_cursor));
    let actions = program_actions(&page.program, delivered, page.next_after_input)?;
    Ok(with_actions(json!(page), actions, Some(0)))
}

pub(crate) fn page(mut page: ProgramPage, capacity: usize) -> Result<Value> {
    if page.inputs.is_empty() {
        let value = page_value(&page)?;
        return within_capacity(value, capacity);
    }
    let mut inputs = std::mem::take(&mut page.inputs);
    let original_next = page.next_after_input;
    page.inputs = vec![inputs[0].clone()];
    page.next_after_input = if inputs.len() > 1 {
        Some(inputs[0].sequence)
    } else {
        original_next
    };
    let count = paging::fitting_prefix(
        &inputs,
        &page_value(&page)?,
        "inputs",
        "next_after_input",
        capacity,
        |prefix, more| {
            let last = prefix.last().expect("nonempty prefix").sequence;
            let next = if more { Some(last) } else { original_next };
            Ok((
                program_actions(&page.program, Some(last), next)?,
                json!(next),
            ))
        },
    )?;
    page.next_after_input = if count < inputs.len() {
        Some(inputs[count - 1].sequence)
    } else {
        original_next
    };
    inputs.truncate(count);
    page.inputs = inputs;
    page_value(&page)
}

pub(crate) fn list_actions(
    programs: &[ProgramSummary],
    next: &Option<String>,
) -> Result<Vec<Value>> {
    let mut actions = Vec::new();
    for program in programs {
        actions.push(action("get_program", json!({"program_id":program.id}))?);
    }
    if let Some(after) = next {
        actions.push(action("list_programs", json!({"after":after,"limit":25}))?);
    }
    actions.push(begin_action()?);
    Ok(actions)
}

pub(crate) fn list(mut list: ProgramList, capacity: usize) -> Result<Value> {
    if list.programs.is_empty() {
        return within_capacity(
            with_actions(
                json!(&list),
                list_actions(&list.programs, &list.next_after)?,
                Some(0),
            ),
            capacity,
        );
    }
    let mut programs = std::mem::take(&mut list.programs);
    let original_next = list.next_after.clone();
    list.programs = vec![programs[0].clone()];
    list.next_after = summary_next(&list.programs, programs.len() > 1, &original_next);
    let first = with_actions(
        json!(&list),
        list_actions(&list.programs, &list.next_after)?,
        Some(0),
    );
    let count = summary_prefix(&programs, &first, capacity, &original_next)?;
    list.next_after = summary_next(&programs[..count], count < programs.len(), &original_next);
    programs.truncate(count);
    list.programs = programs;
    Ok(with_actions(
        json!(&list),
        list_actions(&list.programs, &list.next_after)?,
        Some(0),
    ))
}

pub(crate) fn workspace(state: WorkspaceState, capacity: usize) -> Result<Value> {
    crate::workspace_output::workspace(state, capacity)
}

fn summary_next(
    programs: &[ProgramSummary],
    more: bool,
    original: &Option<String>,
) -> Option<String> {
    if more {
        programs.last().map(|p| p.cursor().encode())
    } else {
        original.clone()
    }
}

fn summary_prefix(
    programs: &[ProgramSummary],
    first: &Value,
    capacity: usize,
    original: &Option<String>,
) -> Result<usize> {
    paging::fitting_prefix(
        programs,
        first,
        "programs",
        "next_after",
        capacity,
        |prefix, more| {
            let next = summary_next(prefix, more, original);
            Ok((list_actions(prefix, &next)?, json!(next)))
        },
    )
}

pub(crate) fn within_capacity(value: Value, capacity: usize) -> Result<Value> {
    if encoded_len(&value)? <= capacity {
        Ok(value)
    } else {
        Err(Error::RequestTooLarge)
    }
}

pub(crate) fn skill() -> Value {
    with_actions(
        json!({"name":"tectd-program","body":PROGRAM_SKILL}),
        Vec::new(),
        None,
    )
}

/// Measures actual MCP string escaping. The input maximum keeps old history readable
/// after a later PRD edit without reading the entire history inside every save.
pub(crate) struct ProgramEncoding {
    pub capacity: usize,
}

impl ProgramOutputGuard for ProgramEncoding {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        let once = serde_json::to_string(input).map_err(|_| Error::TransportUnavailable)?;
        let twice = serde_json::to_string(&once).map_err(|_| Error::TransportUnavailable)?;
        let empty = serde_json::to_string(&"\"\"").expect("empty JSON string");
        i64::try_from(twice.len() - empty.len()).map_err(|_| Error::RequestTooLarge)
    }

    fn check(&self, program: &Program) -> Result<()> {
        if self.capacity > MAX_FRAME_BYTES || program.max_input_bytes < 0 {
            return Err(Error::InvalidArguments);
        }
        let input_cost =
            usize::try_from(program.max_input_bytes).map_err(|_| Error::RequestTooLarge)?;
        for step in [
            ProgramStep::Compose,
            ProgramStep::WaitingInput,
            ProgramStep::Ready,
        ] {
            let mut projection = program.clone();
            projection.current_step = step;
            let sample = ProgramPage {
                program: projection,
                inputs: vec![ProgramInput {
                    id: Uuid::max(),
                    sequence: i64::MAX,
                    request_id: Uuid::max(),
                    session_id: Uuid::max(),
                    input: String::new(),
                }],
                next_after_input: Some(i64::MAX),
            };
            let bytes = encoded_len(&page_value(&sample)?)?
                .checked_add(input_cost)
                .ok_or(Error::RequestTooLarge)?;
            if bytes > self.capacity {
                return Err(Error::RequestTooLarge);
            }
        }
        // A full name must also remain enumerable beside the existing maximum selection.
        // This reserves actual protocol/domain bounds, not an arbitrary name-length cap.
        let mut state = WorkspaceState::opened(
            Workspace {
                id: program.workspace_id,
                key: "w".repeat(128),
            },
            Session {
                id: Uuid::max(),
                workspace_id: program.workspace_id,
                host_id: Uuid::max(),
                native_session_id: Uuid::max().to_string(),
                revoked: false,
            },
        );
        state.setup_context = Some(tect_domain::SetupContext {
            task_directory: "\u{1}".repeat(MAX_SOURCE_PATH_BYTES),
            setup: Some(tect_domain::SetupSummary {
                id: Uuid::max(),
                status: tect_domain::SetupStatus::Draft,
                revision: i64::MAX,
                current_step: tect_domain::SetupStep::WaitingInput,
            }),
        });
        state.programs = vec![program.summary()];
        state.next_after = Some(program.summary().cursor().encode());
        state.selected_worktrees = (0..MAX_WORKTREES)
            .map(|_| WorktreeSummary {
                id: Uuid::max(),
                repository_id: Uuid::max(),
                path: "\u{1}".repeat(MAX_SOURCE_PATH_BYTES),
            })
            .collect();
        let result = workspace(state, self.capacity)?;
        if result["programs_delivery"] == "use_list_programs" {
            Err(Error::RequestTooLarge)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests;
