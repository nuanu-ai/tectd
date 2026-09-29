use super::*;

#[test]
fn generic_stale_manifest_advertises_only_exact_refresh_before_completion() {
    let context = context(PipelineKnowledgeResourceState::Stale, true);
    let values = actions(&context).unwrap();
    assert_eq!(values.len(), 2);
    assert!(
        !values
            .iter()
            .any(|value| value["arguments"]["route"] == "slice.pipeline.phase.complete")
    );
    let refresh = action(&values, "pipeline.knowledge_refresh");
    assert_eq!(
        refresh["arguments"]["params"]["run_id"],
        context.run.id.to_string()
    );
    assert_eq!(refresh["arguments"]["params"]["run_revision"], 7);
    assert_eq!(refresh["arguments"]["params"]["phase_id"], "fixture-phase");
}

#[test]
fn generic_current_selection_supplies_exact_consumed_manifest_guard() {
    let current = context(PipelineKnowledgeResourceState::Current, true);
    let values = actions(&current).unwrap();
    let complete = action(&values, "slice.pipeline.phase.complete");
    let manifest = current.knowledge_resources.as_ref().unwrap();
    assert_eq!(
        complete["arguments"]["params"]["consumed_knowledge"],
        json!({"manifest_id":manifest.id,"digest":manifest.digest})
    );
    let complete_help = crate::api::help(
        crate::api::parse_help(json!({
            "mode":"describe","tool":"command","route":"slice.pipeline.phase.complete"
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(complete["route_contract"], complete_help);
    assert_eq!(
        complete["route_contract"]["params_schema"]["properties"]["output"]["properties"]["producer_context_id"]
            ["type"],
        "string"
    );

    let context_call = action(&values, "slice.pipeline.context");
    let context_help = crate::api::help(
        crate::api::parse_help(json!({
            "mode":"describe","tool":"query","route":"slice.pipeline.context"
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(context_call["route_contract"], context_help);
    assert_eq!(
        context_call["route_contract"]["params_schema"]["properties"]["run_id"]["format"],
        "uuid"
    );

    let inactive = context(PipelineKnowledgeResourceState::Inactive, false);
    let values = actions(&inactive).unwrap();
    assert!(
        action(&values, "slice.pipeline.phase.complete")["arguments"]["params"]
            .get("consumed_knowledge")
            .is_none()
    );
}

#[test]
fn v07_completion_action_omits_backend_owned_proof_echoes() {
    let mut current = context(PipelineKnowledgeResourceState::Current, true);
    current.run.definition_version = "0.7.0-native.k1k5".into();
    current.definition.version = current.run.definition_version.clone();
    for phase in current
        .definition
        .phases
        .iter_mut()
        .chain(current.delivered_phases.iter_mut())
    {
        phase.id = "K4".into();
        phase.required_fields = [
            "red_receipt",
            "changes",
            "green_receipt",
            "anti_pattern_review",
            "deviations",
            "missing_proof",
            "authority_boundary",
            "target_binding",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        phase.output_constraints = vec![
            PipelineOutputConstraint::CommandReceipt {
                field: "red_receipt".into(),
                required_status: "failed_as_expected".into(),
                required_scope: "focused".into(),
                require_nonzero_exit: true,
                target_field: Some("target_binding".into()),
                when_verdict: Some("pass".into()),
            },
            PipelineOutputConstraint::CommandReceipt {
                field: "green_receipt".into(),
                required_status: "passed".into(),
                required_scope: "focused".into(),
                require_nonzero_exit: false,
                target_field: Some("target_binding".into()),
                when_verdict: Some("pass".into()),
            },
        ];
        phase.allowed_verdicts = vec!["pass".into()];
    }
    current.run.current_phase_id = Some("K4".into());
    let values = actions(&current).unwrap();
    let complete = action(&values, "slice.pipeline.phase.complete");
    let params = &complete["arguments"]["params"];
    for field in ["consumed_outputs", "consumed_inputs", "consumed_knowledge"] {
        assert!(params.get(field).is_none(), "unexpected v0.7 {field}");
    }
    let fields = complete["context_input"]["fields"].as_array().unwrap();
    assert!(fields.iter().all(
        |field| !field["path"].as_str().unwrap().ends_with("skill_reads")
            && !field["path"].as_str().unwrap().ends_with("resource_reads")
    ));
    let exact = &complete["next_action_contract"];
    assert_eq!(exact["command"], "slice.pipeline.phase.complete");
    assert_eq!(
        exact["fields_schema"]["required"].as_array().unwrap().len(),
        8
    );
    assert_eq!(
        exact["fields_schema"]["properties"]["red_receipt"]["required_status"],
        "failed_as_expected"
    );
    assert_eq!(
        exact["fields_schema"]["properties"]["red_receipt"]["same_target_as"],
        "target_binding"
    );
    assert_eq!(
        exact["fields_schema"]["properties"]["green_receipt"]["exit_code"],
        "zero"
    );
    assert_eq!(
        exact["call_template"]["arguments"]["params"]["run_revision"],
        7
    );
    assert!(
        exact["backend_owned_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "consumed_outputs")
    );
}

#[test]
fn delivery_is_reused_within_epoch_and_only_explicit_refresh_rereads() {
    let mut current = context(PipelineKnowledgeResourceState::Current, true);
    current.delivery_fresh = false;
    let reused = Delivery::reread(&current, false);
    assert!(!reused.reread);
    assert!(Delivery::reread(&current, true).reread);
    current.delivery_fresh = true;
    assert!(Delivery::reread(&current, false).reread);

    current.run.delivery_mode = PipelineDeliveryMode::Whole;
    current.delivered_phases = vec![phase()];
    restrict_definition_delivery(&mut current, false);
    assert!(current.definition.phases.is_empty());
    assert!(current.delivered_phases.is_empty());
}

#[test]
fn agent_supplied_consumed_digest_does_not_create_backend_delivery_receipt() {
    let current = context(PipelineKnowledgeResourceState::Current, true);
    assert!(current.delivery_receipt.is_none());
    let values = actions(&current).unwrap();
    let complete = action(&values, "slice.pipeline.phase.complete");
    assert!(
        complete["arguments"]["params"]
            .get("consumed_knowledge")
            .is_some()
    );
    assert!(current.delivery_receipt.is_none());
}

#[test]
fn b05_completion_guidance_includes_conditional_research_checkpoint() {
    let mut current = context(PipelineKnowledgeResourceState::Current, false);
    current.run.definition_kind = tect_domain::PipelineKind::DeepBrainstorming;
    current.definition.kind = tect_domain::PipelineKind::DeepBrainstorming;
    current.run.current_phase_id = Some("B05".into());
    current.definition.phases[0].id = "B05".into();
    current.delivered_phases[0].id = "B05".into();
    let values = actions(&current).unwrap();
    let fields = action(&values, "slice.pipeline.phase.complete")["context_input"]["fields"]
        .as_array()
        .unwrap();
    assert!(fields.iter().any(|field| {
        field["path"] == "arguments.params.research_checkpoint"
            && field["format"]
                .as_str()
                .unwrap()
                .starts_with("Required only for waiting_research:")
    }));
}

#[test]
fn open_checkpoint_wait_guidance_keeps_resolution_available_while_knowledge_is_stale() {
    let mut producer = context(PipelineKnowledgeResourceState::Stale, true);
    producer.run.status = PipelineRunStatus::WaitingInput;
    let consumer_run_id = uuid::Uuid::new_v4();
    producer.checkpoints = vec![open_checkpoint(&producer, Some(consumer_run_id))];

    let values = actions(&producer).unwrap();
    assert_eq!(values.len(), 4);
    assert_eq!(values[0]["arguments"]["route"], "slice.pipeline.context");
    assert_eq!(
        values[0]["arguments"]["params"]["run_id"],
        consumer_run_id.to_string()
    );
    let resolve = action(&values, "slice.pipeline.checkpoint.resolve");
    assert_eq!(resolve["kind"], "needs_input");
    assert_eq!(
        resolve["arguments"]["params"]["producer_run_id"],
        producer.run.id.to_string()
    );
    assert_eq!(resolve["arguments"]["params"]["producer_run_revision"], 7);
    assert_eq!(
        resolve["arguments"]["params"]["checkpoint"],
        json!(producer.checkpoints[0].checkpoint)
    );
    assert!(resolve["arguments"]["params"].get("action").is_none());
    assert!(resolve["arguments"]["params"].get("reason").is_none());
    assert!(resolve["arguments"]["params"].get("terminal").is_none());
    assert!(
        resolve["input"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| {
                field["path"] == "arguments.params.terminal"
                    && field["format"]
                        .as_str()
                        .unwrap()
                        .ends_with("Never invent or substitute references.")
            })
    );
    assert!(
        values
            .iter()
            .any(|value| value["arguments"]["route"] == "pipeline.knowledge_refresh")
    );
    assert_eq!(values[3]["arguments"]["route"], "slice.pipeline.context");
    assert_eq!(
        values[3]["arguments"]["params"]["run_id"],
        producer.run.id.to_string()
    );
    assert!(
        !values
            .iter()
            .any(|value| value["arguments"]["route"] == "slice.pipeline.input")
    );
}

#[test]
fn unbound_open_checkpoint_starts_with_scope_candidate_context() {
    let mut producer = context(PipelineKnowledgeResourceState::Current, false);
    producer.run.status = PipelineRunStatus::WaitingInput;
    producer.checkpoints = vec![open_checkpoint(&producer, None)];
    let values = actions(&producer).unwrap();
    assert_eq!(values[0]["arguments"]["route"], "slice.candidates.context");
    assert_eq!(
        values[0]["arguments"]["params"]["scope_id"],
        producer.run.scope_id.to_string()
    );
    assert_eq!(
        values[1]["arguments"]["route"],
        "slice.pipeline.checkpoint.resolve"
    );
}

#[test]
fn unfinished_consumer_with_closed_source_only_points_back_to_planning_context() {
    let mut consumer = context(PipelineKnowledgeResourceState::Current, false);
    let producer_run_id = uuid::Uuid::new_v4();
    let mut checkpoint = open_checkpoint(&consumer, Some(consumer.run.id));
    checkpoint.producer_run_id = producer_run_id;
    checkpoint.status = PipelineCheckpointStatus::Cancelled;
    consumer.source_checkpoint = Some(checkpoint.checkpoint.clone());
    consumer.checkpoints = vec![checkpoint];

    let values = actions(&consumer).unwrap();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["arguments"]["route"], "slice.pipeline.context");
    assert_eq!(
        values[0]["arguments"]["params"]["run_id"],
        producer_run_id.to_string()
    );
    assert_eq!(values[1]["arguments"]["route"], "slice.candidates.context");
    assert!(!values.iter().any(|value| {
        matches!(
            value["arguments"]["route"].as_str(),
            Some("slice.pipeline.phase.complete" | "slice.pipeline.input")
        )
    }));
}
