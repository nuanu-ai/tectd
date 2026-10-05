use super::ledger::{parse_requirements_ledger, violation, wrap_prior_ledger_error};
use super::*;
use std::collections::BTreeMap;

fn navigation_phase(
    id: &str,
    ordinal: u32,
    allowed_backward_to: &[&str],
) -> PipelinePhaseDefinition {
    PipelinePhaseDefinition {
        id: id.into(),
        ordinal,
        title: id.into(),
        required: true,
        disposition_required: false,
        instructions: vec![],
        skills: vec![],
        resources: vec![],
        required_artifacts: vec![],
        validator_contracts: vec![],
        required_fields: vec![],
        allowed_verdicts: vec![],
        required_dispositions: vec![],
        allowed_dispositions: vec![],
        output_constraints: vec![],
        verdict_routes: vec![],
        followup_contracts: vec![],
        allowed_backward_to: allowed_backward_to
            .iter()
            .map(|value| (*value).into())
            .collect(),
        fresh_reviewer_input: false,
        retry_policy: PipelinePhaseRetryPolicy::Repeatable,
        output_contract: String::new(),
    }
}

fn navigation_definition(k3_allowed_backward_to: &[&str]) -> PipelineDefinitionSnapshot {
    PipelineDefinitionSnapshot {
        kind: PipelineKind::LightweightTddDevelopment,
        version: "test".into(),
        digest: "test".into(),
        overview: PipelineInstructionSnapshot {
            id: "test".into(),
            version: "test".into(),
            digest: "test".into(),
            body: String::new(),
            origin_refs: vec![],
        },
        default_mode: PipelineDeliveryMode::Phasewise,
        allowed_modes: vec![PipelineDeliveryMode::Phasewise],
        phases: vec![
            navigation_phase("K1", 1, &[]),
            navigation_phase("K2", 2, &["K1"]),
            navigation_phase("K3", 3, k3_allowed_backward_to),
        ],
        completion_contract: String::new(),
        escalation_contract: String::new(),
        forbidden_claims: vec![],
    }
}

fn waiting_request(revisit_phase_id: Option<&str>) -> CompletePipelinePhase {
    CompletePipelinePhase {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 3,
        phase_id: "K3".into(),
        outcome: PipelinePhaseOutcome::WaitingInput,
        transition: PipelineTransition::Continue,
        output: PipelinePhaseOutputDraft {
            body: String::new(),
            producer_context_id: "test".into(),
            fields: BTreeMap::new(),
            verdict: None,
            dispositions: vec![],
            skill_reads: vec![],
            resource_reads: vec![],
            artifacts: vec![],
            evidence_artifacts: vec![],
            validator_receipts: vec![],
            followup_proposal: None,
            reviewer_context: None,
            reference: None,
            knowledge_publication: None,
        },
        consumed_outputs: vec![],
        consumed_inputs: vec![],
        revisit_phase_id: revisit_phase_id.map(Into::into),
        escalation_target: None,
        terminal_result: None,
        publish_blocked_result: false,
        consumed_knowledge: None,
        research_checkpoint: None,
    }
}

mod ledger;
mod local_execution;
mod navigation;
mod reviewer;
