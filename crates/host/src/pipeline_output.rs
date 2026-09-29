mod actions;

use crate::{response_diet, responses};
use actions::actions;
use serde::Serialize;
use serde_json::{Value, json};
use tect_domain::{
    BeginPipelineRunOutcome, Error, PipelineCheckpointStatus, PipelineContextResponse,
    PipelineKnowledgeResourceState, PipelineKnowledgeState, PipelineMutationOutcome,
    PipelineOutputConstraint, PipelinePhaseDefinition, PipelineRunContext, PipelineRunStatus,
    ResolvePipelineCheckpointOutcome, Result,
};

pub(crate) struct PipelineEncoding {
    capacity: usize,
}

impl PipelineEncoding {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self { capacity }
    }
}

impl tect_application::PipelineExecutionOutputGuard for PipelineEncoding {
    fn check_begin(&self, value: &BeginPipelineRunOutcome) -> Result<()> {
        begin(value.clone(), self.capacity).map(|_| ())
    }

    fn check_mutation(&self, value: &PipelineMutationOutcome) -> Result<()> {
        mutation(value.clone(), self.capacity).map(|_| ())
    }

    fn check_checkpoint_resolution(&self, value: &ResolvePipelineCheckpointOutcome) -> Result<()> {
        checkpoint_resolution(value.clone(), self.capacity).map(|_| ())
    }
}

/// How much of the run context a reply carries; see `response_diet`.
#[derive(Clone)]
struct Delivery {
    reread: bool,
    phase_map: Option<Value>,
    preserve_delivered_phases: bool,
    preserve_outputs: bool,
}

impl Delivery {
    fn reread(context: &PipelineRunContext, refresh: bool) -> Self {
        Self {
            reread: context.delivery_fresh || refresh,
            phase_map: Some(phase_map(context)),
            preserve_delivered_phases: !context.run.definition_version.starts_with("0.7"),
            preserve_outputs: !context.run.definition_version.starts_with("0.7"),
        }
    }

    fn mutation(context: &PipelineRunContext) -> Self {
        Self {
            reread: false,
            phase_map: None,
            preserve_delivered_phases: !context.run.definition_version.starts_with("0.7"),
            preserve_outputs: !context.run.definition_version.starts_with("0.7"),
        }
    }
}

pub(crate) fn begin(mut value: BeginPipelineRunOutcome, capacity: usize) -> Result<Value> {
    let context = match &mut value {
        BeginPipelineRunOutcome::Created(context) | BeginPipelineRunOutcome::Replay(context) => {
            context
        }
    };
    let actions = actions(context)?;
    let mut delivery = Delivery::reread(context, true);
    delivery.preserve_delivered_phases = !context.run.definition_version.starts_with("0.7");
    restrict_definition_delivery(context, true);
    encode(value, actions, capacity, Some(&delivery))
}

pub(crate) fn context(
    value: PipelineContextResponse,
    capacity: usize,
    refresh: bool,
) -> Result<Value> {
    match value {
        PipelineContextResponse::Current(mut context) => {
            let actions = actions(&context)?;
            let delivery = Delivery::reread(&context, refresh);
            restrict_definition_delivery(&mut context, delivery.reread);
            encode_context(*context, actions, capacity, &delivery)
        }
        PipelineContextResponse::Output(output) => {
            let mut actions = vec![responses::action(
                "slice_pipeline_context",
                json!({"run_id":output.run_id}),
            )?];
            crate::api::attach_route_contract(&mut actions[0])?;
            encode(*output, actions, capacity, None)
        }
        PipelineContextResponse::DeliveryReceipt(receipt) => {
            let mut actions = vec![responses::action(
                "slice_pipeline_context",
                json!({"run_id":receipt.run_id,"view":"delivery_receipt"}),
            )?];
            crate::api::attach_route_contract(&mut actions[0])?;
            encode(*receipt, actions, capacity, None)
        }
    }
}

pub(crate) fn instruction(
    value: tect_domain::PipelineInstructionResponse,
    capacity: usize,
) -> Result<Value> {
    encode(value, Vec::new(), capacity, None)
}

pub(crate) fn mutation(mut value: PipelineMutationOutcome, capacity: usize) -> Result<Value> {
    let actions = without_route_contracts(actions(&value.context)?);
    restrict_definition_delivery(&mut value.context, false);
    let delivery = Delivery::mutation(&value.context);
    encode_mutation(value, actions, capacity, &delivery)
}

pub(crate) fn checkpoint_resolution(
    mut value: ResolvePipelineCheckpointOutcome,
    capacity: usize,
) -> Result<Value> {
    let actions = without_route_contracts(actions(&value.context)?);
    restrict_definition_delivery(&mut value.context, false);
    let delivery = Delivery::mutation(&value.context);
    encode(value, actions, capacity, Some(&delivery))
}

/// Ordinal, id and title of every phase; replaces the legacy manifest overview.
fn phase_map(context: &PipelineRunContext) -> Value {
    Value::Array(
        context
            .definition
            .phases
            .iter()
            .map(|phase| json!({"ordinal":phase.ordinal,"id":phase.id,"title":phase.title}))
            .collect(),
    )
}

/// Mutation replies repeat routes the agent has just used; their contracts stay
/// available through begin, context and help.
fn without_route_contracts(mut actions: Vec<Value>) -> Vec<Value> {
    for action in &mut actions {
        if let Some(object) = action.as_object_mut() {
            object.remove("route_contract");
        }
    }
    actions
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

fn with_route_contracts(mut actions: Vec<Value>) -> Result<Vec<Value>> {
    for action in &mut actions {
        crate::api::attach_route_contract(action)?;
    }
    Ok(actions)
}

fn knowledge_is_stale(context: &PipelineRunContext) -> bool {
    matches!(
        context.knowledge_status.as_ref().map(|status| status.state),
        Some(PipelineKnowledgeState::Stale | PipelineKnowledgeState::NeedsContext)
    ) || matches!(
        context
            .knowledge_resource_status
            .as_ref()
            .map(|status| status.state),
        Some(PipelineKnowledgeResourceState::Stale | PipelineKnowledgeResourceState::NeedsContext)
    )
}

fn knowledge_refresh_action(context: &PipelineRunContext) -> Result<Value> {
    let run = &context.run;
    let phase_id = run
        .current_phase_id
        .as_ref()
        .ok_or(Error::InternalInvariant)?;
    responses::action(
        "pipeline_knowledge_refresh",
        json!({"request_id":request_id(run.id,run.revision,"knowledge-refresh"),
            "run_id":run.id,"run_revision":run.revision,"phase_id":phase_id}),
    )
}

fn checkpoint_wait_actions(
    context: &PipelineRunContext,
    checkpoint: &tect_domain::PipelineResearchCheckpoint,
) -> Result<Vec<Value>> {
    let run = &context.run;
    let primary = if let Some(consumer_run_id) = checkpoint.consumer_run_id {
        responses::action("slice_pipeline_context", json!({"run_id":consumer_run_id}))?
    } else {
        responses::action(
            "slice_candidate_context",
            json!({"scope_id":run.scope_id,"view":"overview","limit":25}),
        )?
    };
    let resolve = crate::api::needs_action(
        "needs_input",
        "slice_pipeline_checkpoint_resolve",
        json!({
            "request_id":request_id(run.id,run.revision,"checkpoint-resolve"),
            "producer_run_id":run.id,
            "producer_run_revision":run.revision,
            "checkpoint":checkpoint.checkpoint,
        }),
        "input",
        json!({"fields":[
            {"path":"arguments.params.action","format":"accept or reject only an actual completed bound Research result; cancel closes this wait without an answer. Accept requires fresh producer basis. Use cancel/rework for a stale wait that cannot be accepted."},
            {"path":"arguments.params.reason","format":"Concrete reason for resolving this exact research checkpoint."},
            {"path":"arguments.params.terminal","format":"For accept/reject, copy the exact result_id, output_id and output_digest returned by the bound Research context; omit for cancel. Never invent or substitute references."}
        ]}),
    )?;
    let mut actions = vec![primary, resolve];
    if knowledge_is_stale(context) {
        actions.push(knowledge_refresh_action(context)?);
    }
    actions.push(responses::action(
        "slice_pipeline_context",
        json!({"run_id":run.id}),
    )?);
    Ok(actions)
}

fn encode<T: Serialize>(
    value: T,
    actions: Vec<Value>,
    capacity: usize,
    delivery: Option<&Delivery>,
) -> Result<Value> {
    let mut data = serde_json::to_value(value).map_err(|_| Error::TransportUnavailable)?;
    if let Some(delivery) = delivery
        && let Some(context) = response_diet::pipeline_context_mut(&mut data)
    {
        response_diet::pipeline_context_with_delivery(
            context,
            delivery.reread,
            delivery.phase_map.clone(),
            delivery.preserve_delivered_phases,
            delivery.preserve_outputs,
        );
    }
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
    delivery: &Delivery,
) -> Result<Value> {
    if let Ok(result) = encode(value.clone(), actions.clone(), capacity, Some(delivery)) {
        return Ok(result);
    }
    add_output_actions(&value, &mut actions)?;
    value.outputs.clear();
    value.outputs_complete = false;
    encode(value, actions, capacity, Some(delivery))
}

fn encode_mutation(
    mut value: PipelineMutationOutcome,
    mut actions: Vec<Value>,
    capacity: usize,
    delivery: &Delivery,
) -> Result<Value> {
    if let Ok(result) = encode(value.clone(), actions.clone(), capacity, Some(delivery)) {
        return Ok(result);
    }
    add_output_actions(&value.context, &mut actions)?;
    value.context.outputs.clear();
    value.context.outputs_complete = false;
    encode(value, actions, capacity, Some(delivery))
}

fn add_output_actions(context: &PipelineRunContext, actions: &mut Vec<Value>) -> Result<()> {
    for binding in &context.bindings {
        let mut action = responses::action(
            "slice_pipeline_context",
            json!({"run_id":context.run.id,"view":"output","output_id":binding.output_id,
                "digest":binding.output_digest}),
        )?;
        crate::api::attach_route_contract(&mut action)?;
        actions.push(action);
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

#[cfg(test)]
#[path = "pipeline_output/tests.rs"]
mod tests;
