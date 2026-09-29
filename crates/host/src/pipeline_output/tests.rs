use super::*;
use tect_domain::{
    KnowledgeAccessScope, KnowledgeBindingPurpose, KnowledgeBindingTarget, KnowledgeBindingVersion,
    KnowledgeEpistemicState, KnowledgeKind, KnowledgeLifecycleState, KnowledgeProfileId,
    KnowledgeProfileSections, PipelineCheckpointBasis, PipelineCheckpointRef,
    PipelineCheckpointStatus, PipelineDefinitionSnapshot, PipelineDeliveryMode,
    PipelineInquiryCompletion, PipelineInquiryContract, PipelineInquiryTopicLevel,
    PipelineInstructionResponse, PipelineInstructionSection, PipelineInstructionSnapshot,
    PipelineKnowledgeBindingPin, PipelineKnowledgeResource, PipelineKnowledgeResourceManifest,
    PipelineKnowledgeResourceStatus, PipelinePhaseDefinition, PipelinePhaseRetryPolicy,
    PipelineResearchCheckpoint, PipelineRun, PlanningTaskContext,
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

#[test]
fn instruction_query_output_is_one_snapshot_without_the_full_manifest() {
    let value = super::instruction(
        PipelineInstructionResponse {
            run_id: uuid::Uuid::new_v4(),
            phase_id: Some("fixture-phase".into()),
            section: PipelineInstructionSection::Skill,
            instruction: instruction(),
        },
        64 * 1024,
    )
    .unwrap();

    assert_eq!(value["instruction"]["id"], "fixture:instruction");
    assert_eq!(value["section"], "skill");
    assert!(value.get("definition").is_none());
    assert!(value["actions"].as_array().is_some_and(Vec::is_empty));
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
        delivery_receipt: None,
        delivery_fresh: false,
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

#[path = "tests/action_guidance.rs"]
mod action_guidance;
