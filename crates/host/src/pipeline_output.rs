use crate::{response_diet, responses};
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

fn actions(context: &PipelineRunContext) -> Result<Vec<Value>> {
    let run = &context.run;
    if run.status == PipelineRunStatus::Superseded {
        return with_route_contracts(vec![responses::action(
            "slice_pipeline_context",
            json!({"run_id":run.id}),
        )?]);
    }
    let Some(phase_id) = &run.current_phase_id else {
        return with_route_contracts(vec![responses::action(
            "slice_context",
            json!({"slice_id":run.slice_id}),
        )?]);
    };
    if let Some(source_checkpoint) = &context.source_checkpoint
        && let Some(checkpoint) = context.checkpoints.iter().find(|checkpoint| {
            checkpoint.checkpoint == *source_checkpoint
                && checkpoint.status != PipelineCheckpointStatus::Open
        })
    {
        return with_route_contracts(vec![
            responses::action(
                "slice_pipeline_context",
                json!({"run_id":checkpoint.producer_run_id}),
            )?,
            responses::action(
                "slice_candidate_context",
                json!({"scope_id":run.scope_id,"view":"overview","limit":25}),
            )?,
        ]);
    }
    if matches!(run.status, PipelineRunStatus::WaitingInput)
        && let Some(checkpoint) = context.checkpoints.iter().find(|checkpoint| {
            checkpoint.status == PipelineCheckpointStatus::Open
                && checkpoint.producer_run_id == run.id
                && checkpoint.producer_phase_id == *phase_id
        })
    {
        return with_route_contracts(checkpoint_wait_actions(context, checkpoint)?);
    }
    if knowledge_is_stale(context) {
        return with_route_contracts(vec![
            knowledge_refresh_action(context)?,
            responses::action("slice_pipeline_context", json!({"run_id":run.id}))?,
        ]);
    }
    let action = match run.status {
        PipelineRunStatus::Active => {
            let legacy_definition = !run.definition_version.starts_with("0.7");
            let mut params = json!({"request_id":request_id(run.id,run.revision,"complete"),
                "run_id":run.id,"run_revision":run.revision,"phase_id":phase_id});
            let mut fields = vec![
                json!({"path":"arguments.params.outcome","format":"Caller-reported phase outcome allowed by the current verdict route."}),
                json!({"path":"arguments.params.transition","format":"Transition allowed by the current verdict route."}),
                json!({"path":"arguments.params.output","format":"Complete phase output with producer context, typed fields, verdict, exact route dispositions, artifacts, validator receipts, any route-required follow-up proposal, and reviewer attestation/reference. A bounded body is optional for v0.7. Read phase.instructions, skills, and resources as guidance; the backend records delivery and dependency proof."}),
                json!({"path":"arguments.params.terminal_result","format":"Required only for a terminal complete, published block, or pipeline-kind escalation."}),
            ];
            if legacy_definition {
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
                params["consumed_outputs"] = json!(consumed_outputs);
                params["consumed_inputs"] = json!(consumed_inputs);
                let legacy = context
                    .knowledge
                    .as_ref()
                    .filter(|manifest| !manifest.selected.is_empty())
                    .map(|manifest| (manifest.id, manifest.digest.as_str()));
                let generic = context
                    .knowledge_resources
                    .as_ref()
                    .filter(|manifest| !manifest.selected.is_empty())
                    .map(|manifest| (manifest.id, manifest.digest.as_str()));
                if let Some((manifest_id, digest)) = legacy.or(generic) {
                    params["consumed_knowledge"] =
                        json!({"manifest_id":manifest_id,"digest":digest});
                }
                fields.push(json!({"path":"arguments.params.output.skill_reads","format":"After reading the current phase's skills, submit exactly its skills entries as {instruction_id: id, version, digest}. Include no entries from instructions or resources. Use [] when skills is empty."}));
                fields.push(json!({"path":"arguments.params.output.resource_reads","format":"After reading the current phase's resources, submit exactly its resources entries as {instruction_id: id, version, digest}. Include no entries from instructions or skills. Use [] when resources is empty."}));
                fields[2] = json!({"path":"arguments.params.output","format":"Complete phase output body, producer context, typed fields, verdict, exact route dispositions, pinned skill/resource reads, artifacts, validator receipts, any route-required follow-up proposal, and reviewer attestation/reference. Read phase.instructions as guidance; they have no receipt array. Artifact digests are SHA-256 of the exact submitted UTF-8 body bytes. Preserve the supplied consumed_outputs, consumed_inputs, and consumed_knowledge parameters. If consumed_knowledge is absent in this action, leave it absent; an empty knowledge manifest is not a consumption receipt."});
            }
            let current = context
                .delivered_phases
                .iter()
                .find(|phase| &phase.id == phase_id)
                .or_else(|| {
                    context
                        .definition
                        .phases
                        .iter()
                        .find(|phase| &phase.id == phase_id)
                });
            if current.is_some_and(|phase| {
                phase.output_constraints.iter().any(|constraint| {
                    matches!(
                        constraint,
                        PipelineOutputConstraint::ResolvedKnowledgePublication { .. }
                    )
                })
            }) {
                fields.push(json!({"path":"arguments.params.output.knowledge_publication",
                    "format":"For promoted verdicts only: exact backend-issued change_id, publisher_receipt_id, publisher_receipt_digest, and covered operation_ids. The backend resolves producer lineage; unrelated or forged receipts fail."}));
            }
            if run.definition_kind == tect_domain::PipelineKind::DeepBrainstorming
                && phase_id == "B05"
            {
                fields.push(json!({"path":"arguments.params.research_checkpoint",
                    "format":"Required only for waiting_research: question, answer_criteria, research inquiry and reason for a separate Research Slice. Use the delivered B05 method and exact current basis; omit for other verdicts."}));
            }
            let mut action = crate::api::needs_action(
                "needs_context",
                "slice_pipeline_phase_complete",
                params.clone(),
                "context_input",
                json!({"fields":fields}),
            )?;
            if let Some(phase) = current {
                action["next_action_contract"] = phase_completion_contract(phase, &params);
            }
            action
        }
        PipelineRunStatus::WaitingInput | PipelineRunStatus::Blocked => crate::api::needs_action(
            "needs_input",
            "slice_pipeline_input",
            json!({"request_id":request_id(run.id,run.revision,"input"),
                "run_id":run.id,"run_revision":run.revision,"phase_id":phase_id}),
            "input",
            json!({"fields":[
                {"path":"arguments.params.input","format":"Exact phase-local operator answer, context, direct authority instruction, or resume input."},
                {"path":"arguments.params.source_amendment","format":"Optional Full Design source amendment. Supply the exact current non-stale phase-5 output/binding/artifact/source identity, a changed hash-valid successor source artifact whose name equals its path, target phase slice-component-decision-interrogator, and the direct authority scope and provenance. The backend records it as phase-5 input lineage and returns phase 5 current; do not ask for confirmation again when the exact input already grants authority."}
            ]}),
        )?,
        PipelineRunStatus::Completed
        | PipelineRunStatus::Escalated
        | PipelineRunStatus::Superseded => unreachable!(),
    };
    with_route_contracts(vec![
        action,
        responses::action("slice_pipeline_context", json!({"run_id":run.id}))?,
    ])
}

include!("pipeline_output/phase_completion_contract.rs");

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
