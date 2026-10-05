mod actions;
pub(crate) mod receipt_diff;

use crate::{response_diet, responses};
#[cfg(test)]
use actions::actions;
use actions::actions_for;
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
    fn check_context(&self, value: &PipelineRunContext) -> Result<()> {
        compact_encode(value, value, true, self.capacity).map(|_| ())
    }
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

pub(crate) fn begin(value: BeginPipelineRunOutcome, capacity: usize) -> Result<Value> {
    let context = match &value {
        BeginPipelineRunOutcome::Created(context) | BeginPipelineRunOutcome::Replay(context) => {
            context
        }
    };
    compact_encode(&value, context, false, capacity)
}
pub(crate) fn context(
    value: PipelineContextResponse,
    capacity: usize,
    _refresh: bool,
) -> Result<Value> {
    match value {
        PipelineContextResponse::Current(context) => {
            compact_encode(&*context, &context, true, capacity)
        }
        PipelineContextResponse::Output(output) => {
            output_read(&output, capacity, Default::default())
        }
        PipelineContextResponse::DeliveryReceipt(receipt) => sized(
            serde_json::to_value(*receipt).map_err(|_| Error::TransportUnavailable)?,
            Vec::new(),
            capacity,
        ),
        _ => Err(Error::InvalidArguments),
    }
}

#[cfg(test)]
pub(crate) fn instruction(
    value: tect_domain::PipelineInstructionResponse,
    capacity: usize,
) -> Result<Value> {
    instruction_read(&value, capacity, Default::default(), None)
}

pub(crate) fn context_pinned(
    value: PipelineContextResponse,
    capacity: usize,
    query: &tect_domain::PipelineRunContextQuery,
) -> Result<Value> {
    let window = crate::json_fragment::Window {
        offset_bytes: query.offset_bytes,
        limit_bytes: query.limit_bytes,
        representation_digest: query.representation_digest.as_deref(),
    };
    let mut params = serde_json::to_value(query).map_err(|_| Error::TransportUnavailable)?;
    params
        .as_object_mut()
        .ok_or(Error::InternalInvariant)?
        .retain(|_, value| !value.is_null());
    match value {
        PipelineContextResponse::Output(output) => output_read(&output, capacity, window),
        PipelineContextResponse::Snapshot(read) => crate::json_fragment::encode(
            &*read,
            Vec::new(),
            capacity,
            window,
            json!({"run_id":read.run_id,"definition_digest":read.definition_digest}),
            "slice_pipeline_context",
            params,
        ),
        PipelineContextResponse::PhaseContract(read) => crate::json_fragment::encode(
            &*read,
            Vec::new(),
            capacity,
            window,
            json!({"run_id":read.run_id,"definition_digest":read.definition_digest,"phase_id":read.phase.id}),
            "slice_pipeline_context",
            params,
        ),
        PipelineContextResponse::ReceiptDiff(read) => {
            receipt_diff::encode(&read, capacity, query, window)
        }
        PipelineContextResponse::Details(read) => crate::json_fragment::encode(
            &*read,
            Vec::new(),
            capacity,
            window,
            json!({"run_id":read.run_id,"run_revision":read.run_revision,"section":read.section}),
            "slice_pipeline_context",
            params,
        ),
        other => context(other, capacity, query.refresh),
    }
}
pub(crate) fn instruction_pinned(
    value: tect_domain::PipelineInstructionResponse,
    capacity: usize,
    query: &tect_domain::PipelineInstructionQuery,
) -> Result<Value> {
    instruction_read(
        &value,
        capacity,
        crate::json_fragment::Window {
            offset_bytes: query.offset_bytes,
            limit_bytes: query.limit_bytes,
            representation_digest: query.representation_digest.as_deref(),
        },
        query.phase_id.as_deref(),
    )
}
fn output_read(
    value: &tect_domain::PipelinePhaseOutput,
    capacity: usize,
    window: crate::json_fragment::Window<'_>,
) -> Result<Value> {
    let action = responses::action("slice_pipeline_context", json!({"run_id":value.run_id}))?;
    let pins =
        json!({"run_id":value.run_id,"view":"output","output_id":value.id,"digest":value.digest});
    crate::json_fragment::encode(
        value,
        vec![action],
        capacity,
        window,
        pins.clone(),
        "slice_pipeline_context",
        pins,
    )
}
fn instruction_read(
    value: &tect_domain::PipelineInstructionResponse,
    capacity: usize,
    window: crate::json_fragment::Window<'_>,
    phase_id: Option<&str>,
) -> Result<Value> {
    let pins = json!({"run_id":value.run_id,"instruction_id":value.instruction.id,"version":value.instruction.version,"digest":value.instruction.digest,"refresh":true});
    let params = if let Some(phase_id) = phase_id {
        json!({"run_id":value.run_id,"phase_id":phase_id})
    } else {
        pins.clone()
    };
    crate::json_fragment::encode(
        value,
        Vec::new(),
        capacity,
        window,
        pins.clone(),
        "slice_pipeline_instruction",
        params,
    )
}

pub(crate) fn mutation(value: PipelineMutationOutcome, capacity: usize) -> Result<Value> {
    compact_encode(&value, &value.context, false, capacity)
}
pub(crate) fn checkpoint_resolution(
    value: ResolvePipelineCheckpointOutcome,
    capacity: usize,
) -> Result<Value> {
    compact_encode(&value, &value.context, false, capacity)
}
fn with_route_contracts(actions: Vec<Value>) -> Result<Vec<Value>> {
    Ok(actions)
}

/// Every lifecycle response uses one projection and an exact fixed set of reads.
fn compact_encode<T: Serialize>(
    value: &T,
    context: &PipelineRunContext,
    fresh_current: bool,
    capacity: usize,
) -> Result<Value> {
    let mut data = serde_json::to_value(value).map_err(|_| Error::TransportUnavailable)?;
    let compact = response_diet::pipeline_context_mut(&mut data).ok_or(Error::InternalInvariant)?;
    response_diet::compact_pipeline(compact);
    if let Some(object) = data.as_object_mut() {
        if let Some(result) = object.remove("result") {
            object.insert(
                "result_reference".into(),
                json!({"result_id":result["id"],"view":"details","section":"history"}),
            );
        }
        if let Some(checkpoint) = object.remove("checkpoint") {
            object.insert("checkpoint_reference".into(),json!({"checkpoint":checkpoint["checkpoint"],"status":checkpoint["status"],"view":"details","section":"history"}));
        }
    }
    let mut available = actions_for(context, fresh_current)?;
    for action in &mut available {
        if let Some(object) = action.as_object_mut() {
            object.remove("route_contract");
            object.remove("next_action_contract");
        }
    }
    let run = &context.run;
    available.push(responses::action(
        "slice_pipeline_context",
        json!({"run_id":run.id,"view":"snapshot","definition_digest":run.definition_digest}),
    )?);
    if let Some(phase_id) = &run.current_phase_id {
        available.push(responses::action("slice_pipeline_context",json!({"run_id":run.id,"view":"phase_contract","definition_digest":run.definition_digest,"phase_id":phase_id}))?);
    }
    available.push(responses::action(
        "slice_pipeline_context",
        json!({"run_id":run.id,"view":"details","run_revision":run.revision,"section":"all"}),
    )?);
    if let Some(action) = available.first()
        && let Some(mut help) = crate::api::schema_help_action(
            action["tool"].as_str().ok_or(Error::InternalInvariant)?,
            &action["arguments"],
        )?
    {
        help.as_object_mut()
            .ok_or(Error::InternalInvariant)?
            .remove("route_contract");
        available.push(help);
    }
    match sized(data.clone(), available.clone(), capacity) {
        Ok(value) => Ok(value),
        Err(Error::RequestTooLarge) => {
            // Only a size failure permits moving legacy consumption parameters.
            for action in &mut available {
                let Some(params) = action
                    .pointer_mut("/arguments/params")
                    .and_then(Value::as_object_mut)
                else {
                    continue;
                };
                let moved = params.remove("consumed_outputs").is_some()
                    | params.remove("consumed_inputs").is_some()
                    | params.remove("consumed_knowledge").is_some();
                if moved {
                    action["kind"] = json!("needs_context");
                    action["context_input"] = json!({"kind":"context_input","fields":[{"path":"arguments.params.consumed_outputs","format":"Read pinned details section inputs for this exact run_revision and copy its consumed_outputs array before submitting."},{"path":"arguments.params.consumed_inputs","format":"Read pinned details section inputs for this exact run_revision and copy its consumed_inputs array before submitting."},{"path":"arguments.params.consumed_knowledge","format":"Read pinned details section inputs for this exact run_revision; copy consumed_knowledge only when non-null, otherwise omit this parameter."},{"path":"arguments.params.output","format":"Read the exact pinned phase_contract; supply its required output and transition."}]});
                }
            }
            available.push(responses::action("slice_pipeline_context",json!({"run_id":run.id,"view":"details","run_revision":run.revision,"section":"inputs"}))?);
            sized(data, available, capacity)
        }
        Err(error) => Err(error),
    }
}
fn sized(data: Value, actions: Vec<Value>, capacity: usize) -> Result<Value> {
    let value = responses::with_actions(data, actions, Some(0));
    if responses::encoded_len(&value)? > capacity.min(crate::json_fragment::READ_BUDGET) {
        Err(Error::RequestTooLarge)
    } else {
        Ok(value)
    }
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
