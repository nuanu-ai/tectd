use super::*;
use tect_domain::{
    KnowledgeAccessScope, KnowledgeBindingPurpose, KnowledgeBindingTarget, KnowledgeBindingVersion,
    KnowledgeEpistemicState, KnowledgeKind, KnowledgeLifecycleState, KnowledgeProfileId,
    KnowledgeProfileSections, PipelineCheckpointBasis, PipelineCheckpointRef,
    PipelineCheckpointStatus, PipelineDefinitionSnapshot, PipelineDeliveryMode,
    PipelineInquiryCompletion, PipelineInquiryContract, PipelineInquiryTopicLevel,
    PipelineInstructionSnapshot, PipelineKnowledgeBindingPin, PipelineKnowledgeResource,
    PipelineKnowledgeResourceManifest, PipelineKnowledgeResourceStatus, PipelinePhaseDefinition,
    PipelinePhaseRetryPolicy, PipelineResearchCheckpoint, PipelineRun, PlanningTaskContext,
};

fn instruction() -> PipelineInstructionSnapshot {
    PipelineInstructionSnapshot {
        id: "fixture:instruction".into(),
        version: "1".into(),
        digest: "instruction-digest".into(),
        body: "fixture instruction".into(),
        origin_refs: vec!["fixture".into()],
    }
}

fn phase() -> PipelinePhaseDefinition {
    PipelinePhaseDefinition {
        id: "fixture-phase".into(),
        ordinal: 1,
        title: "Fixture phase".into(),
        required: true,
        disposition_required: false,
        instructions: vec![instruction()],
        skills: Vec::new(),
        resources: Vec::new(),
        required_artifacts: Vec::new(),
        validator_contracts: Vec::new(),
        required_fields: Vec::new(),
        allowed_verdicts: Vec::new(),
        required_dispositions: Vec::new(),
        allowed_dispositions: Vec::new(),
        output_constraints: Vec::new(),
        verdict_routes: Vec::new(),
        followup_contracts: Vec::new(),
        allowed_backward_to: Vec::new(),
        fresh_reviewer_input: false,
        retry_policy: PipelinePhaseRetryPolicy::Repeatable,
        output_contract: "fixture output".into(),
    }
}

fn resource() -> PipelineKnowledgeResource {
    PipelineKnowledgeResource {
        unit_id: uuid::Uuid::new_v4(),
        revision: 1,
        lifecycle: KnowledgeLifecycleState::Active,
        access_scope: KnowledgeAccessScope::WorkspaceMembers,
        rdf_digest: "rdf-digest".into(),
        unit_iri: "urn:fixture:unit".into(),
        revision_iri: "urn:fixture:revision".into(),
        title: "Fixture knowledge".into(),
        canonical_text: "Fixture canonical text.".into(),
        knowledge_kind: KnowledgeKind::Constraint,
        epistemic_state: KnowledgeEpistemicState::Normative,
        target_iris: vec!["urn:fixture:target".into()],
        profiles: vec![KnowledgeProfileId::General],
        conditions: Vec::new(),
        exceptions: Vec::new(),
        sections: KnowledgeProfileSections::default(),
        inquiry_briefs: None,
        source_pins: Vec::new(),
        latest_validation: None,
        binding: PipelineKnowledgeBindingPin {
            binding_iri: "urn:fixture:binding".into(),
            target: KnowledgeBindingTarget::Workspace,
            purpose: KnowledgeBindingPurpose::Required,
            version_resolution: KnowledgeBindingVersion::CurrentAccepted,
            definition_kind: None,
            definition_version: None,
            definition_digest: None,
        },
        why_included: "workspace_binding".into(),
    }
}

fn context(state: PipelineKnowledgeResourceState, selected: bool) -> PipelineRunContext {
    let run_id = uuid::Uuid::new_v4();
    let phase = phase();
    PipelineRunContext {
        run: PipelineRun {
            id: run_id,
            scope_id: uuid::Uuid::new_v4(),
            slice_id: uuid::Uuid::new_v4(),
            slice_revision: 1,
            revision: 7,
            definition_kind: tect_domain::PipelineKind::LightweightTddDevelopment,
            definition_version: "fixture-version".into(),
            definition_digest: "definition-digest".into(),
            delivery_mode: PipelineDeliveryMode::Phasewise,
            qualification_reason: "fixture".into(),
            status: PipelineRunStatus::Active,
            current_phase_id: Some(phase.id.clone()),
            current_phase_ordinal: Some(phase.ordinal),
        },
        definition: PipelineDefinitionSnapshot {
            kind: tect_domain::PipelineKind::LightweightTddDevelopment,
            version: "fixture-version".into(),
            digest: "definition-digest".into(),
            overview: instruction(),
            default_mode: PipelineDeliveryMode::Phasewise,
            allowed_modes: vec![PipelineDeliveryMode::Phasewise],
            phases: vec![phase.clone()],
            completion_contract: "fixture completion".into(),
            escalation_contract: "fixture escalation".into(),
            forbidden_claims: Vec::new(),
        },
        inquiry: None,
        source_checkpoint: None,
        checkpoints: Vec::new(),
        delivered_phases: vec![phase],
        attempts: Vec::new(),
        bindings: Vec::new(),
        outputs: Vec::new(),
        outputs_complete: true,
        inputs: Vec::new(),
        result: None,
        knowledge: None,
        knowledge_status: None,
        knowledge_resources: Some(PipelineKnowledgeResourceManifest {
            id: uuid::Uuid::new_v4(),
            digest: "manifest-digest".into(),
            semantic_digest: "semantic-digest".into(),
            workspace_generation: 1,
            run_id,
            run_revision: if matches!(
                state,
                PipelineKnowledgeResourceState::Stale
                    | PipelineKnowledgeResourceState::NeedsContext
            ) {
                6
            } else {
                7
            },
            phase_id: "fixture-phase".into(),
            definition_version: "fixture-version".into(),
            definition_digest: "definition-digest".into(),
            method_requirements: Vec::new(),
            inquiry: None,
            projection_policy: None,
            selected: selected.then(resource).into_iter().collect(),
            unresolved_needs: Vec::new(),
            freshness_warnings: Vec::new(),
        }),
        knowledge_resource_status: Some(PipelineKnowledgeResourceStatus {
            state,
            current_generation: 1,
            changed_unit_ids: Vec::new(),
            freshness_warnings: Vec::new(),
            access_changed: false,
        }),
    }
}

fn action<'a>(values: &'a [Value], route: &str) -> &'a Value {
    values
        .iter()
        .find(|value| value["arguments"]["route"] == route)
        .expect("expected action route")
}

fn open_checkpoint(
    context: &PipelineRunContext,
    consumer_run_id: Option<uuid::Uuid>,
) -> PipelineResearchCheckpoint {
    PipelineResearchCheckpoint {
        checkpoint: PipelineCheckpointRef {
            checkpoint_id: uuid::Uuid::new_v4(),
            digest: "checkpoint-digest".into(),
        },
        status: PipelineCheckpointStatus::Open,
        producer_run_id: context.run.id,
        producer_run_revision: context.run.revision,
        producer_definition_version: context.run.definition_version.clone(),
        producer_definition_digest: context.run.definition_digest.clone(),
        producer_phase_id: context.run.current_phase_id.clone().unwrap(),
        producer_output_id: uuid::Uuid::new_v4(),
        producer_output_revision: 1,
        producer_output_digest: "producer-output-digest".into(),
        basis: PipelineCheckpointBasis {
            consumed_outputs: Vec::new(),
            consumed_inputs: Vec::new(),
            consumed_knowledge: None,
        },
        question: "Which research result resolves this decision?".into(),
        answer_criteria: "An exact bound result.".into(),
        inquiry: PipelineInquiryContract {
            topic_level: PipelineInquiryTopicLevel::Scope,
            task_context: PlanningTaskContext::default(),
            completion: PipelineInquiryCompletion::Research {
                allow_inconclusive: true,
            },
        },
        reason: "Separate research is required.".into(),
        consumer_run_id,
        consumer_result_id: None,
        consumer_terminal_output_id: None,
        consumer_terminal_output_digest: None,
        resolution_action: None,
        resolution_reason: None,
    }
}

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
