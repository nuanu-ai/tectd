use super::*;
use std::collections::BTreeMap;
use uuid::Uuid;

pub(super) fn proof_test_definition(version: &str) -> PipelineDefinitionSnapshot {
    let instruction = PipelineInstructionSnapshot {
        id: "instruction".into(),
        version: "1".into(),
        digest: "instruction-digest".into(),
        body: "instruction".into(),
        origin_refs: Vec::new(),
    };
    let phase = PipelinePhaseDefinition {
        id: "phase-1".into(),
        ordinal: 1,
        title: "Phase".into(),
        required: true,
        disposition_required: false,
        instructions: vec![instruction.clone()],
        skills: Vec::new(),
        resources: Vec::new(),
        required_artifacts: Vec::new(),
        validator_contracts: Vec::new(),
        followup_contracts: Vec::new(),
        required_fields: Vec::new(),
        allowed_verdicts: Vec::new(),
        required_dispositions: Vec::new(),
        allowed_dispositions: Vec::new(),
        output_constraints: Vec::new(),
        verdict_routes: Vec::new(),
        allowed_backward_to: Vec::new(),
        fresh_reviewer_input: false,
        retry_policy: PipelinePhaseRetryPolicy::Repeatable,
        output_contract: "contract".into(),
    };
    PipelineDefinitionSnapshot {
        kind: PipelineKind::LightweightTddDevelopment,
        version: version.into(),
        digest: "definition-digest".into(),
        overview: instruction,
        default_mode: PipelineDeliveryMode::Phasewise,
        allowed_modes: vec![PipelineDeliveryMode::Phasewise],
        phases: vec![phase],
        completion_contract: "completion".into(),
        escalation_contract: "escalation".into(),
        forbidden_claims: Vec::new(),
    }
}

pub(super) fn proof_test_completion() -> CompletePipelinePhase {
    CompletePipelinePhase {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: "phase-1".into(),
        outcome: PipelinePhaseOutcome::Completed,
        transition: PipelineTransition::Continue,
        output: PipelinePhaseOutputDraft {
            body: "body".into(),
            producer_context_id: "ctx".into(),
            fields: BTreeMap::new(),
            verdict: None,
            dispositions: Vec::new(),
            skill_reads: Vec::new(),
            resource_reads: Vec::new(),
            artifacts: Vec::new(),
            evidence_artifacts: Vec::new(),
            validator_receipts: Vec::new(),
            followup_proposal: None,
            knowledge_publication: None,
            reviewer_context: None,
            reference: None,
        },
        consumed_outputs: vec![PipelineConsumedOutput {
            phase_id: "previous".into(),
            output_revision: 1,
            digest: "output-digest".into(),
        }],
        consumed_inputs: vec![PipelineConsumedInput {
            input_id: Uuid::new_v4(),
            sequence: 1,
            digest: "input-digest".into(),
        }],
        revisit_phase_id: None,
        escalation_target: None,
        terminal_result: None,
        publish_blocked_result: false,
        consumed_knowledge: None,
        research_checkpoint: None,
    }
}

pub(super) fn request(body: String) -> RecordPipelineInput {
    RecordPipelineInput {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 2,
        phase_id: "slice-implementation-spec-synthesizer".to_owned(),
        input: "Direct source amendment authority.".to_owned(),
        source_amendment: Some(PipelineSourceAmendment {
            target_phase_id: "slice-component-decision-interrogator".to_owned(),
            predecessor: PipelineSourcePredecessor {
                output_id: Uuid::new_v4(),
                output_revision: 1,
                output_digest: "a".repeat(64),
                artifact_name: "requirements-ledger.json".to_owned(),
                artifact_digest: "b".repeat(64),
                source_path: "source.md".to_owned(),
                source_digest: "c".repeat(64),
            },
            successor: PipelineSourceSuccessor {
                path: "source.md".to_owned(),
                artifact: PipelineSourceArtifactDraft {
                    name: "source.md".to_owned(),
                    media_type: "text/markdown".to_owned(),
                    body,
                    digest: "d67e2e944994496c8d8ec76eed0cf9f09679448d584b532bebf941852a37f5ed"
                        .to_owned(),
                    reference: None,
                },
            },
            authorization_scope: "Amend the current Full Design source.".to_owned(),
            authorization_provenance: "Exact direct operator input.".to_owned(),
        }),
    }
}

pub(super) fn assert_source_amendment_refusal(result: Result<()>, rule: &str, field: &str) {
    let refusal = result.unwrap_err().refusal().unwrap();
    assert_eq!(refusal.code, RefusalCode::InputSchemaInvalid);
    assert_eq!(refusal.rule.as_deref(), Some(rule));
    assert_eq!(
        refusal.path.as_deref(),
        Some(format!("arguments.params.source_amendment.{field}").as_str())
    );
}

mod amendment;
mod completion;
mod receipts;
