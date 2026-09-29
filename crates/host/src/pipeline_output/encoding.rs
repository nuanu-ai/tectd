use super::Delivery;
use crate::{response_diet, responses};
use serde::Serialize;
use serde_json::{Value, json};
use tect_domain::{Error, PipelineMutationOutcome, PipelineRunContext, Result};

pub(super) fn encode<T: Serialize>(
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
    if let Some(context) = response_diet::pipeline_context_mut(&mut data) {
        project_paged_context(context);
    }
    if let Some(context) = response_diet::pipeline_context_mut(&mut data)
        && let Some(paged) = context
            .as_object_mut()
            .and_then(|object| object.remove("knowledge_resources_paged"))
    {
        context["knowledge_resources"] = paged;
    }
    let result = responses::with_actions(data, actions, Some(0));
    if responses::encoded_len(&result)? > capacity {
        Err(Error::RequestTooLarge)
    } else {
        Ok(result)
    }
}

fn project_paged_context(context: &mut Value) {
    if context
        .pointer("/knowledge_resources_paged/contract_version")
        .and_then(Value::as_str)
        != Some("dk-2-paged")
    {
        return;
    }
    let Some(run_id) = context.pointer("/run/id").cloned() else {
        return;
    };
    context["response_contract_version"] = json!("slice.begin.paged.v1");
    context["body_delivery"] = json!("pinned_references");
    if let Some(overview) = context.pointer_mut("/definition/overview") {
        project_instruction(overview, &run_id);
    }
    if let Some(phases) = context
        .pointer_mut("/definition/phases")
        .and_then(Value::as_array_mut)
    {
        for phase in phases {
            for section in ["instructions", "skills", "resources"] {
                if let Some(instructions) = phase.get_mut(section).and_then(Value::as_array_mut) {
                    for instruction in instructions {
                        project_instruction(instruction, &run_id);
                    }
                }
            }
        }
    }
    if let Some(delivered) = context
        .get_mut("delivered_phases")
        .and_then(Value::as_array_mut)
    {
        for phase in delivered {
            *phase = json!({"id":phase.get("id"),"ordinal":phase.get("ordinal"),"delivery":"pinned_references"});
        }
    }
}

fn project_instruction(instruction: &mut Value, run_id: &Value) {
    let (Some(id), Some(version), Some(digest)) = (
        instruction.get("id").cloned(),
        instruction.get("version").cloned(),
        instruction.get("digest").cloned(),
    ) else {
        return;
    };
    *instruction = json!({"id":id,"version":version,"digest":digest,
    "read":{"route":"slice.pipeline.instruction","params":{
        "run_id":run_id,"instruction_id":id,"version":version,"digest":digest,"refresh":true
    }}});
}

pub(super) fn encode_context(
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

pub(super) fn encode_mutation(
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
