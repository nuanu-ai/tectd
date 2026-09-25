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
    for field in &phase.required_fields {
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
            "required":phase.required_fields
        },
        "verdict_routes":phase.verdict_routes,
        "call_template":{"tool":"command","arguments":{"route":"slice.pipeline.phase.complete","params":params}}
    })
}
