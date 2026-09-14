use std::collections::BTreeMap;
use tect_domain::{
    CompletePipelinePhase, PipelineDefinitionSnapshot, PipelineDeliveryMode,
    PipelineInstructionSnapshot, PipelineKind, PipelineOutputConstraint, PipelinePhaseDefinition,
    PipelinePhaseOutcome, PipelinePhaseOutputDraft, PipelinePhaseRetryPolicy,
    PipelineSkillReadReceipt, PipelineTransition, PipelineVerdictRoute,
};
use uuid::Uuid;

pub(super) fn instruction(id: &str, digest: &str) -> PipelineInstructionSnapshot {
    PipelineInstructionSnapshot {
        id: id.into(),
        version: "v1".into(),
        digest: digest.into(),
        body: format!("Exact body for {id}"),
        origin_refs: vec![format!("v1:{id}")],
    }
}

pub(super) fn definition() -> PipelineDefinitionSnapshot {
    PipelineDefinitionSnapshot {
        kind: PipelineKind::LightweightTddDevelopment,
        version: "v1".into(),
        digest: "definition-digest".into(),
        overview: instruction("pipeline-overview", "overview-digest"),
        default_mode: PipelineDeliveryMode::Whole,
        allowed_modes: vec![PipelineDeliveryMode::Whole, PipelineDeliveryMode::Phasewise],
        phases: vec![PipelinePhaseDefinition {
            id: "slice-tdd-cycle-runner".into(),
            ordinal: 1,
            title: "TDD cycle runner".into(),
            required: true,
            disposition_required: true,
            instructions: vec![instruction("slice-tdd-cycle-runner", "instruction-digest")],
            skills: vec![instruction(
                "superpowers:test-driven-development",
                "skill-digest",
            )],
            resources: vec![],
            required_artifacts: vec![],
            validator_contracts: vec![],
            followup_contracts: vec![],
            required_fields: vec![
                "red_observation".into(),
                "red_exit_code".into(),
                "red_failure_observed".into(),
                "green_observation".into(),
                "green_exit_code".into(),
                "green_pass_observed".into(),
                "selected_test_target".into(),
                "target_binding".into(),
            ],
            allowed_verdicts: vec![
                "implemented_locally".into(),
                "blocked_missing_target".into(),
            ],
            allowed_dispositions: vec!["red_green_refactor_proof".into(), "test_target_gap".into()],
            required_dispositions: vec![],
            output_constraints: vec![
                PipelineOutputConstraint::FieldIntegerNotEquals {
                    field: "red_exit_code".into(),
                    value: 0,
                    when_verdict: Some("implemented_locally".into()),
                },
                PipelineOutputConstraint::FieldBooleanEquals {
                    field: "red_failure_observed".into(),
                    value: true,
                    when_verdict: Some("implemented_locally".into()),
                },
                PipelineOutputConstraint::FieldIntegerEquals {
                    field: "green_exit_code".into(),
                    value: 0,
                    when_verdict: Some("implemented_locally".into()),
                },
                PipelineOutputConstraint::FieldBooleanEquals {
                    field: "green_pass_observed".into(),
                    value: true,
                    when_verdict: Some("implemented_locally".into()),
                },
                PipelineOutputConstraint::FieldsEqual {
                    field: "selected_test_target".into(),
                    other_field: "target_binding".into(),
                    when_verdict: Some("implemented_locally".into()),
                },
            ],
            verdict_routes: vec![
                PipelineVerdictRoute {
                    verdict: "implemented_locally".into(),
                    outcome: PipelinePhaseOutcome::Completed,
                    transition: PipelineTransition::Continue,
                    dispositions: vec!["red_green_refactor_proof".into()],
                    revisit_to: vec![],
                },
                PipelineVerdictRoute {
                    verdict: "blocked_missing_target".into(),
                    outcome: PipelinePhaseOutcome::Blocked,
                    transition: PipelineTransition::Continue,
                    dispositions: vec!["test_target_gap".into()],
                    revisit_to: vec![],
                },
            ],
            allowed_backward_to: vec![],
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "Record meaningful RED and bounded GREEN evidence.".into(),
        }],
        completion_contract: "The required phase is complete with exact evidence.".into(),
        escalation_contract: "Escalation preserves the unfinished boundary.".into(),
        forbidden_claims: vec!["unverified completion".into()],
    }
}

pub(super) fn valid_completion() -> CompletePipelinePhase {
    CompletePipelinePhase {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: "slice-tdd-cycle-runner".into(),
        outcome: PipelinePhaseOutcome::Completed,
        transition: PipelineTransition::Continue,
        output: PipelinePhaseOutputDraft {
            body: "RED failed for the intended reason; bounded GREEN passed.".into(),
            producer_context_id: "producer-session-a".into(),
            fields: BTreeMap::from([
                ("red_observation".into(), "focused assertion failed".into()),
                ("red_exit_code".into(), "1".into()),
                ("red_failure_observed".into(), "true".into()),
                (
                    "green_observation".into(),
                    "focused assertion passed".into(),
                ),
                ("green_exit_code".into(), "0".into()),
                ("green_pass_observed".into(), "true".into()),
                ("selected_test_target".into(), "preview::focused".into()),
                ("target_binding".into(), "preview::focused".into()),
            ]),
            verdict: Some("implemented_locally".into()),
            dispositions: vec!["red_green_refactor_proof".into()],
            skill_reads: vec![PipelineSkillReadReceipt {
                instruction_id: "superpowers:test-driven-development".into(),
                version: "v1".into(),
                digest: "skill-digest".into(),
            }],
            resource_reads: vec![],
            artifacts: vec![],
            validator_receipts: vec![],
            followup_proposal: None,
            knowledge_publication: None,
            reviewer_context: None,
            reference: Some("tdd-notes.md".into()),
        },
        consumed_outputs: vec![],
        consumed_inputs: vec![],
        consumed_knowledge: None,
        revisit_phase_id: None,
        escalation_target: None,
        terminal_result: None,
        publish_blocked_result: false,
    }
}
