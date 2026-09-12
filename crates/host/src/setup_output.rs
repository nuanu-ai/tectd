use crate::program_output::{begin_action, input_action, within_capacity};
use crate::responses::{action, encoded_len, with_actions};
use serde_json::{Value, json};
use tect_application::{ProgramOutputGuard, SetupOutputGuard};
use tect_domain::{
    AppliedSetup, Error, FileObservation, FilePublication, PublicationOutcome, Result, Setup,
    SetupFileStatus, SetupInput, SetupPage, SetupStatus, SetupStep,
};
use uuid::Uuid;

const SETUP_SKILL: &str = include_str!("../../../skills/tectd-setup/SKILL.md");

pub(crate) fn reload(id: Uuid) -> Result<Value> {
    action(
        "get_setup",
        json!({"setup_id":id,"after_input":0,"limit":25}),
    )
}

fn save_action(setup: &Setup, cursor: i64) -> Result<Value> {
    crate::api::needs_action(
        "needs_input",
        "save_setup",
        json!({"setup_id":setup.id,
        "revision":setup.revision,"input_cursor":cursor,"ready":false}),
        "input",
        json!({"fields":[
            {"path":"arguments.params.content","format":"Whole proposed AGENTS.md as an optional string-or-null patch; omission preserves and null clears."},
            {"path":"arguments.params.working_notes","format":"Optional string-or-null continuation patch."},
            {"path":"arguments.params.pending_question","format":"Optional string-or-null question patch."},
            {"path":"arguments.params.ready","format":"Required boolean already set false in this template; set true only for coherent content with all inputs incorporated and no pending question."}
        ]}),
    )
}

fn actions(setup: &Setup, delivered: Option<i64>, next: Option<i64>) -> Result<Vec<Value>> {
    let mut calls = vec![crate::api::method_action("tectd-setup")?];
    if let Some(after_input) = next {
        calls.push(action(
            "get_setup",
            json!({"setup_id":setup.id,"after_input":after_input,"limit":25}),
        )?);
    } else if delivered.is_none() && setup.input_cursor < setup.latest_input {
        calls.push(action(
            "get_setup",
            json!({"setup_id":setup.id,"after_input":setup.input_cursor,"limit":25}),
        )?);
    }
    match setup.current_step {
        SetupStep::Compose => calls.push(save_action(
            setup,
            delivered
                .unwrap_or(setup.input_cursor)
                .max(setup.input_cursor),
        )?),
        SetupStep::WaitingInput => calls.push(input_action(
            "record_setup_input",
            json!({"setup_id":setup.id,"revision":setup.revision,"request_id":Uuid::new_v4()}),
        )?),
        SetupStep::ReadyToApply if next.is_none() => calls.push(action(
            "apply_setup",
            json!({"setup_id":setup.id,"revision":setup.revision}),
        )?),
        SetupStep::Complete => {}
        SetupStep::ReadyToApply => {}
    }
    calls.push(action("list_programs", json!({"limit":25}))?);
    calls.push(begin_action()?);
    Ok(calls)
}

fn instruction(step: SetupStep) -> &'static str {
    match step {
        SetupStep::Compose => {
            "Read the setup skill and all required original inputs. Compose one useful AGENTS.md for this company/work context. Preserve the complete original input, distinguish facts from assumptions, and save the draft, notes and any necessary question before yielding. Show the whole proposed file. Do not ask the human to choose the task directory."
        }
        SetupStep::WaitingInput => {
            "The partial file, notes and exact pending question are durable. Read the setup skill and returned original history before asking only the unresolved question. Record the complete original reply using the supplied identity and revision; then incorporate it and clear or replace the question. Do not repeat settled questions."
        }
        SetupStep::ReadyToApply => {
            "Before calling execute route setup.apply, send the entire saved setup.content verbatim to the user in a commentary code block. The tool result alone and a display after application do not satisfy this step. If this exact content was already shown, do not duplicate it. All inputs are incorporated and no question is pending; then apply the exact ready revision under existing setup authorization without an extra approval ceremony. On an uncertain result, retry the same setup_id and ready revision. Report creation only after current byte verification succeeds."
        }
        SetupStep::Complete => {
            "The saved setup is historically applied. Only an observation marked observed_now verifies the file now; a saved status alone does not. Report current verification and link the file; the whole saved content remains in this response and need not be repeated if already shown. Never recreate a removed or changed file from this historical status. Continue with an existing Program or create a new Program last."
        }
    }
}

fn base(setup: &Setup, file: Value, delivered: Option<i64>, next: Option<i64>) -> Result<Value> {
    Ok(with_actions(
        json!({"setup":setup,"task_directory":setup.directory.path,"file":file,
        "current_step_instruction":instruction(setup.current_step)}),
        actions(setup, delivered, next)?,
        Some(0),
    ))
}

pub(crate) fn saved(setup: Setup) -> Result<Value> {
    base(
        &setup,
        json!({"status":"context_unknown","observed_now":false,
        "reason":"file_not_inspected_by_this_call"}),
        None,
        None,
    )
}

fn page_value(page: &SetupPage) -> Result<Value> {
    let mut file = json!(&page.file);
    file["observed_now"] = json!(true);
    let delivered = Some(
        page.inputs
            .last()
            .map_or(page.setup.input_cursor, |entry| entry.sequence),
    );
    let mut value = base(&page.setup, file, delivered, page.next_after_input)?;
    value["inputs"] = json!(&page.inputs);
    value["next_after_input"] = json!(page.next_after_input);
    Ok(value)
}

pub(crate) fn page(mut page: SetupPage, capacity: usize) -> Result<Value> {
    if page.inputs.is_empty() {
        return within_capacity(page_value(&page)?, capacity);
    }
    let mut inputs = std::mem::take(&mut page.inputs);
    let original_next = page.next_after_input;
    page.inputs = vec![inputs[0].clone()];
    page.next_after_input = if inputs.len() > 1 {
        Some(inputs[0].sequence)
    } else {
        original_next
    };
    let count = crate::program_output::paging::fitting_prefix(
        &inputs,
        &page_value(&page)?,
        "inputs",
        "next_after_input",
        capacity,
        |prefix, more| {
            let last = prefix.last().expect("nonempty prefix").sequence;
            let next = if more { Some(last) } else { original_next };
            Ok((actions(&page.setup, Some(last), next)?, json!(next)))
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

pub(crate) fn applied(result: AppliedSetup) -> Result<Value> {
    base(
        &result.setup,
        json!({"status":"existing","observed_now":true,
        "publication":result.publication}),
        Some(result.setup.input_cursor),
        None,
    )
}

pub(crate) fn skill() -> Value {
    with_actions(
        json!({"name":"tectd-setup","body":SETUP_SKILL}),
        Vec::new(),
        None,
    )
}

/// Reserve the largest whole original input beside every future saved draft and step.
pub(crate) struct SetupEncoding {
    pub capacity: usize,
}
impl SetupOutputGuard for SetupEncoding {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        crate::program_output::ProgramEncoding {
            capacity: self.capacity,
        }
        .input_bytes(input)
    }
    fn check(&self, setup: &Setup) -> Result<()> {
        if self.capacity > crate::frame::MAX_FRAME_BYTES || setup.max_input_bytes < 0 {
            return Err(Error::InvalidArguments);
        }
        let cost = usize::try_from(setup.max_input_bytes).map_err(|_| Error::RequestTooLarge)?;
        for step in [
            SetupStep::Compose,
            SetupStep::WaitingInput,
            SetupStep::ReadyToApply,
            SetupStep::Complete,
        ] {
            let mut projection = setup.clone();
            projection.current_step = step;
            projection.status = if step == SetupStep::Complete {
                SetupStatus::Applied
            } else {
                SetupStatus::Draft
            };
            projection.revision = i64::MAX;
            projection.input_cursor = i64::MAX;
            projection.latest_input = i64::MAX;
            projection.applied_from_revision = Some(i64::MAX);
            projection.applied_sha256 = Some("f".repeat(64));
            within_capacity(saved(projection.clone())?, self.capacity)?;
            let sample = SetupPage {
                setup: projection.clone(),
                inputs: vec![SetupInput {
                    id: Uuid::max(),
                    sequence: i64::MAX,
                    request_id: Uuid::max(),
                    session_id: Uuid::max(),
                    input: String::new(),
                }],
                next_after_input: Some(i64::MAX),
                file: FileObservation {
                    status: SetupFileStatus::Unavailable,
                    byte_length: Some(u64::MAX),
                    sha256: Some("f".repeat(64)),
                    reason: Some("file_changed_during_inspection".into()),
                },
            };
            for next in [None, Some(i64::MAX)] {
                let mut sample = sample.clone();
                sample.next_after_input = next;
                if encoded_len(&page_value(&sample)?)?
                    .checked_add(cost)
                    .ok_or(Error::RequestTooLarge)?
                    > self.capacity
                {
                    return Err(Error::RequestTooLarge);
                }
            }
            within_capacity(
                applied(AppliedSetup {
                    setup: projection,
                    publication: FilePublication {
                        outcome: PublicationOutcome::AlreadyMatches,
                        sha256: "f".repeat(64),
                        byte_length: u64::MAX,
                    },
                })?,
                self.capacity,
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
