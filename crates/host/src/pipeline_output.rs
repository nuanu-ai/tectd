use crate::responses;
use serde::Serialize;
use serde_json::{Value, json};
use tect_domain::{
    BeginPipelineRunOutcome, Error, PipelineContextResponse, PipelineMutationOutcome,
    PipelineRunContext, PipelineRunStatus, Result,
};

pub(crate) fn begin(mut value: BeginPipelineRunOutcome, capacity: usize) -> Result<Value> {
    let actions = actions(match &value {
        BeginPipelineRunOutcome::Created(context) | BeginPipelineRunOutcome::Replay(context) => {
            context
        }
    })?;
    let context = match &mut value {
        BeginPipelineRunOutcome::Created(context) | BeginPipelineRunOutcome::Replay(context) => {
            context
        }
    };
    restrict_definition_delivery(context, true);
    encode(value, actions, capacity)
}

pub(crate) fn context(value: PipelineContextResponse, capacity: usize) -> Result<Value> {
    match value {
        PipelineContextResponse::Current(mut context) => {
            let actions = actions(&context)?;
            restrict_definition_delivery(&mut context, true);
            encode_context(*context, actions, capacity)
        }
        PipelineContextResponse::Output(output) => {
            let actions = vec![responses::action(
                "slice_pipeline_context",
                json!({"run_id":output.run_id}),
            )?];
            encode(*output, actions, capacity)
        }
    }
}

pub(crate) fn mutation(mut value: PipelineMutationOutcome, capacity: usize) -> Result<Value> {
    let actions = actions(&value.context)?;
    restrict_definition_delivery(&mut value.context, false);
    encode_mutation(value, actions, capacity)
}

fn restrict_definition_delivery(context: &mut PipelineRunContext, explicit_reread: bool) {
    match context.run.delivery_mode {
        tect_domain::PipelineDeliveryMode::Phasewise => {
            context.definition.phases = context.delivered_phases.clone();
        }
        tect_domain::PipelineDeliveryMode::Whole if !explicit_reread => {
            context.definition.phases.clear();
            context.delivered_phases.clear();
        }
        tect_domain::PipelineDeliveryMode::Whole => {}
    }
}

fn actions(context: &PipelineRunContext) -> Result<Vec<Value>> {
    let run = &context.run;
    let Some(phase_id) = &run.current_phase_id else {
        return Ok(vec![responses::action(
            "slice_context",
            json!({"slice_id":run.slice_id}),
        )?]);
    };
    let action = match run.status {
        PipelineRunStatus::Active => {
            let ordinal = run.current_phase_ordinal.ok_or(Error::InternalInvariant)?;
            let consumed_outputs = context
                .bindings
                .iter()
                .filter(|binding| binding.phase_ordinal < ordinal && !binding.stale)
                .map(|binding| {
                    json!({"phase_id":binding.phase_id,
                    "output_revision":binding.output_revision,"digest":binding.output_digest})
                })
                .collect::<Vec<_>>();
            let consumed_inputs = context
                .inputs
                .iter()
                .filter(|input| &input.phase_id == phase_id)
                .map(|input| {
                    json!({"input_id":input.id,"sequence":input.sequence,
                    "digest":input.digest})
                })
                .collect::<Vec<_>>();
            crate::api::needs_action(
                "needs_context",
                "slice_pipeline_phase_complete",
                json!({"request_id":request_id(run.id,run.revision,"complete"),
                    "run_id":run.id,"run_revision":run.revision,"phase_id":phase_id,
                    "consumed_outputs":consumed_outputs,"consumed_inputs":consumed_inputs}),
                "context_input",
                json!({"fields":[
                    {"path":"arguments.params.outcome","format":"Caller-reported phase outcome allowed by the current verdict route."},
                    {"path":"arguments.params.transition","format":"Transition allowed by the current verdict route."},
                    {"path":"arguments.params.output","format":"Complete phase output body, producer context, typed fields, verdict, exact route dispositions, pinned skill/resource reads, artifacts, validator receipts, any route-required follow-up proposal, and reviewer attestation/reference."},
                    {"path":"arguments.params.terminal_result","format":"Required only for a terminal complete, published block, or pipeline-kind escalation."}
                ]}),
            )?
        }
        PipelineRunStatus::WaitingInput | PipelineRunStatus::Blocked => crate::api::needs_action(
            "needs_input",
            "slice_pipeline_input",
            json!({"request_id":request_id(run.id,run.revision,"input"),
                "run_id":run.id,"run_revision":run.revision,"phase_id":phase_id}),
            "input",
            json!({"fields":[{"path":"arguments.params.input","format":"Exact phase-local operator answer, context, authority evidence, or resume input."}]}),
        )?,
        PipelineRunStatus::Completed | PipelineRunStatus::Escalated => unreachable!(),
    };
    Ok(vec![
        action,
        responses::action("slice_pipeline_context", json!({"run_id":run.id}))?,
    ])
}

fn encode<T: Serialize>(value: T, actions: Vec<Value>, capacity: usize) -> Result<Value> {
    let data = serde_json::to_value(value).map_err(|_| Error::TransportUnavailable)?;
    let result = responses::with_actions(data, actions, Some(0));
    if responses::encoded_len(&result)? > capacity {
        Err(Error::RequestTooLarge)
    } else {
        Ok(result)
    }
}

fn encode_context(
    mut value: PipelineRunContext,
    mut actions: Vec<Value>,
    capacity: usize,
) -> Result<Value> {
    if let Ok(result) = encode(value.clone(), actions.clone(), capacity) {
        return Ok(result);
    }
    add_output_actions(&value, &mut actions)?;
    value.outputs.clear();
    value.outputs_complete = false;
    encode(value, actions, capacity)
}

fn encode_mutation(
    mut value: PipelineMutationOutcome,
    mut actions: Vec<Value>,
    capacity: usize,
) -> Result<Value> {
    if let Ok(result) = encode(value.clone(), actions.clone(), capacity) {
        return Ok(result);
    }
    add_output_actions(&value.context, &mut actions)?;
    value.context.outputs.clear();
    value.context.outputs_complete = false;
    encode(value, actions, capacity)
}

fn add_output_actions(context: &PipelineRunContext, actions: &mut Vec<Value>) -> Result<()> {
    for binding in &context.bindings {
        actions.push(responses::action(
            "slice_pipeline_context",
            json!({"run_id":context.run.id,"view":"output","output_id":binding.output_id,
                "digest":binding.output_digest}),
        )?);
    }
    Ok(())
}

fn request_id(id: uuid::Uuid, revision: i64, operation: &str) -> uuid::Uuid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("tectd-pipeline:{id}:{revision}:{operation}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}
