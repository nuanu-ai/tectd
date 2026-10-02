use super::*;
use crate::pipeline_definitions::StaticPipelineDefinitions;
use sha2::{Digest, Sha256};
use tect_application::PipelineDefinitionProvider;
use tect_domain::{CompletePipelinePhase, PipelineKind};

fn completion_action(definition: &PipelineDefinitionSnapshot, ordinal: usize) -> Value {
    let phase = definition.phases[ordinal - 1].clone();
    let mut current = context(PipelineKnowledgeResourceState::Inactive, false);
    current.run.definition_kind = definition.kind;
    current.run.definition_version = definition.version.clone();
    current.run.definition_digest = definition.digest.clone();
    current.run.current_phase_id = Some(phase.id.clone());
    current.run.current_phase_ordinal = Some(phase.ordinal);
    current.definition = definition.clone();
    current.delivered_phases = vec![phase];
    actions(&current).unwrap().remove(0)
}

fn local_params(phase: &PipelinePhaseDefinition) -> Value {
    let mut fields = phase
        .required_fields
        .iter()
        .map(|key| (key.clone(), json!("Explicit synthetic local fixture")))
        .collect::<serde_json::Map<_, _>>();
    for (key, value) in [
        ("route", "result_local_only"),
        ("highest_validated_truth", "local_verified"),
        ("terminal_state_candidate", "completed_local_verified"),
        ("deployment_required", "false"),
        ("disposition", "proof_gate"),
    ] {
        fields.insert(key.into(), json!(value));
    }
    let body = "Synthetic local validation fixture";
    let digest = Sha256::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    json!({"request_id":uuid::Uuid::new_v4(),"run_id":uuid::Uuid::new_v4(),"run_revision":16,"phase_id":phase.id,"outcome":"completed","transition":"continue","output":{"body":body,"producer_context_id":"synthetic-local-gate","fields":fields,"verdict":"completed_local_verified","dispositions":["proof_gate"],"artifacts":[{"name":"deployment-validation.md","media_type":"text/markdown","body":body,"digest":digest}]}})
}

fn public_completion(params: Value) -> CompletePipelinePhase {
    let call = crate::api::decode_public_call(
        "command",
        json!({"route":"slice.pipeline.phase.complete","params":params}),
    )
    .unwrap();
    match crate::pipeline_tools::parse(call.name, call.arguments).unwrap() {
        crate::pipeline_tools::PipelineInvocation::Complete(request) => *request,
        _ => panic!("completion bridge changed invocation"),
    }
}

#[test]
fn native4_local_completion_projects_legal_string_fields_and_exact_predicates() {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::FullDesignToExecution)
        .unwrap();
    assert_eq!(definition.version, "0.6.0-native.engineering.4");
    assert_eq!(
        definition.digest,
        "85ec63bae1903fedb0c86ecd5326380ea8d524fe0ee29c5dce6e90b9a30cdd3d"
    );
    let phase = &definition.phases[15];
    let action = completion_action(&definition, 16);
    let contract = &action["next_action_contract"];
    let schema = &contract["fields_schema"];
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["deployment_required"]["type"],
        "string"
    );
    assert_eq!(schema["required"], json!(phase.required_fields));
    assert_eq!(
        contract["output_constraints"],
        json!(phase.output_constraints)
    );
    assert!(
        contract["call_template"]["arguments"]["params"]["output"]["fields"]
            .get("deployment_required")
            .is_some()
    );
    let params = local_params(phase);
    public_completion(params.clone())
        .validate(&definition)
        .unwrap();
    let mut missing = params.clone();
    missing["output"]["fields"]
        .as_object_mut()
        .unwrap()
        .remove("deployment_required");
    assert!(public_completion(missing).validate(&definition).is_err());
    let mut wrong_string = params.clone();
    wrong_string["output"]["fields"]["deployment_required"] = json!("true");
    assert!(
        public_completion(wrong_string)
            .validate(&definition)
            .is_err()
    );
    let mut wrong_bool = params.clone();
    wrong_bool["output"]["fields"]["deployment_required"] = json!(false);
    assert!(
        crate::api::decode_public_call(
            "command",
            json!({"route":"slice.pipeline.phase.complete","params":wrong_bool})
        )
        .is_err()
    );
    let mut handoff = params.clone();
    handoff["output"]["verdict"] = json!("handoff_required");
    handoff["output"]["dispositions"] = json!(["handoff_selected"]);
    handoff["output"]["fields"]["route"] = json!("user_handoff");
    handoff["output"]["fields"]
        .as_object_mut()
        .unwrap()
        .remove("deployment_required");
    public_completion(handoff).validate(&definition).unwrap();

    // Export actual projected schema/candidates for independent JSON Schema
    // validation without introducing a runtime/test dependency into the repo.
    let mut unknown = params["output"]["fields"].clone();
    unknown["undeclared"] = json!("not allowed");
    let mut boolean = params["output"]["fields"].clone();
    boolean["deployment_required"] = json!(false);
    let mut missing_base = params["output"]["fields"].clone();
    missing_base
        .as_object_mut()
        .unwrap()
        .remove("source_artifact");
    eprintln!(
        "ACTION_SCHEMA_PROOF:{}",
        json!({"schema":schema,"cases":[{"name":"local-string-false","fields":params["output"]["fields"],"accept":true},{"name":"unknown","fields":unknown,"accept":false},{"name":"wrong-json-bool","fields":boolean,"accept":false},{"name":"missing-unconditional","fields":missing_base,"accept":false}]})
    );
}

#[test]
fn conditional_declarations_remain_conditional_and_archives_keep_closed_fields() {
    let mut current = context(PipelineKnowledgeResourceState::Inactive, false);
    for phase in current
        .definition
        .phases
        .iter_mut()
        .chain(current.delivered_phases.iter_mut())
    {
        phase.required_fields = vec!["base".into()];
        phase.allowed_verdicts = vec!["pass".into(), "other".into()];
        phase.output_constraints = vec![
            PipelineOutputConstraint::FieldRequired {
                field: "conditional".into(),
                when_verdict: Some("pass".into()),
            },
            PipelineOutputConstraint::FieldRequired {
                field: "always".into(),
                when_verdict: None,
            },
            PipelineOutputConstraint::FieldsEqual {
                field: "conditional".into(),
                other_field: "base".into(),
                when_verdict: Some("pass".into()),
            },
        ];
    }
    let action = actions(&current).unwrap().remove(0);
    assert_eq!(
        action["next_action_contract"]["fields_schema"]["required"],
        json!(["base", "always"])
    );
    assert_eq!(
        action["next_action_contract"]["fields_schema"]["properties"]["conditional"]["type"],
        "string"
    );
    assert_eq!(
        action["next_action_contract"]["output_constraints"],
        json!(current.definition.phases[0].output_constraints)
    );
    for version in ["0.6.0-native.engineering.2", "0.6.0-native.engineering.3"] {
        let definition = StaticPipelineDefinitions
            .definition_for(PipelineKind::FullDesignToExecution, Some(version))
            .unwrap();
        let action = completion_action(&definition, 16);
        let schema = &action["next_action_contract"]["fields_schema"];
        assert_eq!(
            schema["required"],
            json!(definition.phases[15].required_fields)
        );
        assert_eq!(
            schema["properties"].as_object().unwrap().len(),
            definition.phases[15].required_fields.len()
        );
        assert!(schema["properties"].get("deployment_required").is_none());
        assert_eq!(schema["additionalProperties"], false);
    }
}
