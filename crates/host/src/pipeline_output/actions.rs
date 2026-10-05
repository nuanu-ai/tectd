use super::*;

#[cfg(test)]
pub(super) fn actions(context: &PipelineRunContext) -> Result<Vec<Value>> {
    actions_for(context, true)
}

pub(super) fn actions_for(context: &PipelineRunContext, fresh_current: bool) -> Result<Vec<Value>> {
    let run = &context.run;
    if run.status == PipelineRunStatus::Superseded {
        return with_route_contracts(vec![responses::action(
            "slice_context",
            json!({"slice_id":run.slice_id}),
        )?]);
    }
    if tect_domain::is_retired_lightweight(&context.definition) {
        if !fresh_current
            || matches!(
                run.status,
                PipelineRunStatus::Completed | PipelineRunStatus::Escalated
            )
        {
            return with_route_contracts(vec![responses::action(
                "slice_pipeline_context",
                json!({"run_id":run.id}),
            )?]);
        }
        return with_route_contracts(vec![responses::action(
            "slice_pipeline_run_migrate",
            json!({
                "request_id":request_id(run.id,run.revision,"retirement-migrate"),
                "predecessor_run_id":run.id,"expected_revision":run.revision,
                "idempotency_key":format!("retire:{}:{}:{}",run.id,run.revision,tect_domain::CURRENT_LIGHTWEIGHT_VERSION),
                "successor_definition_version":tect_domain::CURRENT_LIGHTWEIGHT_VERSION,"mappings":[]
            }),
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

fn phase_completion_contract(phase: &PipelinePhaseDefinition, mechanical: &Value) -> Value {
    let receipt_schema = json!({
        "type":"object",
        "additionalProperties":false,
        "properties":{
            "command":{"type":"string","minLength":1},
            "target":{"type":"string","minLength":1},
            "status":{"type":"string","enum":["failed_as_expected","passed"]},
            "exit_code":{"type":"integer"},
            "fresh":{"const":true},
            "skipped":{"const":false},
            "scopes":{"type":"array","items":{"type":"string","enum":["focused","affected"]},"minItems":1,"uniqueItems":true}
        },
        "required":["command","target","status","exit_code","fresh","skipped","scopes"]
    });
    let mut properties = serde_json::Map::new();
    let mut values = serde_json::Map::new();
    let mut required = phase.required_fields.clone();
    let mut declared = required.clone();
    for constraint in &phase.output_constraints {
        if let PipelineOutputConstraint::FieldRequired {
            field,
            when_verdict,
        } = constraint
        {
            if !declared.contains(field) {
                declared.push(field.clone());
            }
            if when_verdict.is_none() && !required.contains(field) {
                required.push(field.clone());
            }
        }
    }
    for field in &declared {
        let command_constraint = phase.output_constraints.iter().find_map(|constraint| {
            if let PipelineOutputConstraint::CommandReceipt {
                field: constrained,
                required_status,
                required_scope,
                require_nonzero_exit,
                target_field,
                ..
            } = constraint
                && constrained == field
            {
                Some((
                    required_status,
                    required_scope,
                    require_nonzero_exit,
                    target_field,
                ))
            } else {
                None
            }
        });
        if let Some((status, scope, nonzero, target_field)) = command_constraint {
            properties.insert(
                field.clone(),
                json!({
                    "type":"string",
                    "contentMediaType":"application/json",
                    "decoded_schema":receipt_schema.clone(),
                    "required_status":status,
                    "required_scope":scope,
                    "exit_code":if *nonzero { "nonzero" } else { "zero" },
                    "same_target_as":target_field
                }),
            );
            values.insert(
                field.clone(),
                json!(format!(
                    "<content:{field}: JSON string matching decoded_schema>"
                )),
            );
        } else {
            properties.insert(field.clone(), json!({"type":"string","minLength":1}));
            values.insert(field.clone(), json!(format!("<content:{field}>")));
        }
    }
    let mut params = mechanical.clone();
    params["outcome"] = json!("<content:outcome selected from current verdict route>");
    params["transition"] = json!("<content:transition selected from current verdict route>");
    params["output"] = json!({
        "producer_context_id":"<content:current producer context id>",
        "fields":Value::Object(values),
        "verdict":format!("<content:verdict one of {}>", phase.allowed_verdicts.join("|")),
        "dispositions":["<content:exact disposition for selected verdict route>"]
    });
    json!({
        "command":"slice.pipeline.phase.complete",
        "route":"slice.pipeline.phase.complete",
        "required_params":["request_id","run_id","run_revision","phase_id","outcome","transition","output"],
        "backend_owned_fields":["consumed_outputs","consumed_inputs","consumed_knowledge","output.skill_reads","output.resource_reads"],
        "fields_schema":{
            "type":"object","additionalProperties":false,
            "properties":Value::Object(properties),
            "required":required
        },
        // The field-map schema cannot inspect the sibling output.verdict or
        // compare field values. Preserve exact predicates without making a
        // conditional requirement/value global for unrelated verdicts.
        "output_constraints":phase.output_constraints,
        "verdict_routes":phase.verdict_routes,
        "call_template":{"tool":"command","arguments":{"route":"slice.pipeline.phase.complete","params":params}}
    })
}
