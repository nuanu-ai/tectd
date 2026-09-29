use super::*;

#[test]
fn archived_snapshot_reads_validate_but_do_not_cover_updated_definition() {
    use std::collections::BTreeMap;
    use tect_domain::{
        CompletePipelinePhase, PipelinePhaseOutcome, PipelinePhaseOutputDraft,
        PipelineSkillReadReceipt, PipelineTransition,
    };
    use uuid::Uuid;

    fn completion_for(
        mut definition: PipelineDefinitionSnapshot,
    ) -> (PipelineDefinitionSnapshot, CompletePipelinePhase) {
        let mut phase = definition
            .phases
            .iter()
            .find(|phase| phase.id == "slice-tdd-cycle-runner")
            .unwrap()
            .clone();
        phase.required_fields.clear();
        phase.allowed_verdicts.clear();
        phase.required_dispositions.clear();
        phase.allowed_dispositions.clear();
        phase.disposition_required = false;
        phase.output_constraints.clear();
        phase.required_artifacts.clear();
        phase.validator_contracts.clear();
        phase.verdict_routes.clear();
        phase.followup_contracts.clear();
        phase.fresh_reviewer_input = false;
        let reads = |values: &[tect_domain::PipelineInstructionSnapshot]| {
            values
                .iter()
                .map(|value| PipelineSkillReadReceipt {
                    instruction_id: value.id.clone(),
                    version: value.version.clone(),
                    digest: value.digest.clone(),
                })
                .collect()
        };
        let completion = CompletePipelinePhase {
            request_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            run_revision: 1,
            phase_id: phase.id.clone(),
            outcome: PipelinePhaseOutcome::Completed,
            transition: PipelineTransition::Continue,
            output: PipelinePhaseOutputDraft {
                body: "bounded read-set proof".into(),
                producer_context_id: "test-context".into(),
                fields: BTreeMap::new(),
                verdict: None,
                dispositions: vec![],
                skill_reads: reads(&phase.skills),
                resource_reads: reads(&phase.resources),
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
            revisit_phase_id: None,
            escalation_target: None,
            terminal_result: None,
            publish_blocked_result: false,
            consumed_knowledge: None,
            research_checkpoint: None,
        };
        definition.phases = vec![phase];
        (definition, completion)
    }

    let old = load(
        include_str!("../../../pipeline-definitions/lightweight-tdd-0.1.0-native.1.json"),
        PipelineKind::LightweightTddDevelopment,
    )
    .unwrap();
    let (old, old_completion) = completion_for(old);
    assert!(old_completion.validate(&old).is_ok());

    let current = StaticPipelineDefinitions
        .definition(PipelineKind::LightweightTddDevelopment)
        .unwrap();
    let (current, mut current_completion) = completion_for(current);
    assert!(current_completion.validate(&current).is_ok());
    current_completion.output.skill_reads = old_completion.output.skill_reads;
    assert!(current_completion.validate(&current).is_err());

    let (_, mut omitted_resource) = completion_for(current.clone());
    omitted_resource.output.resource_reads.pop();
    assert!(omitted_resource.validate(&current).is_err());
}

#[test]
fn v07_rejects_agent_supplied_proof_and_legacy_payloads_still_decode() {
    use std::collections::BTreeMap;
    use tect_domain::{
        CompletePipelinePhase, ConsumedKnowledgeManifestRef, PipelineConsumedInput,
        PipelineConsumedOutput, PipelinePhaseOutcome, PipelinePhaseOutputDraft,
        PipelineSkillReadReceipt, PipelineTransition,
    };
    use uuid::Uuid;

    let mut v07 = lightweight_v07().unwrap();
    v07.phases.truncate(1);
    let phase = &mut v07.phases[0];
    phase.ordinal = 1;
    phase.required_fields.clear();
    phase.allowed_verdicts.clear();
    phase.required_dispositions.clear();
    phase.allowed_dispositions.clear();
    phase.disposition_required = false;
    phase.output_constraints.clear();
    phase.required_artifacts.clear();
    phase.validator_contracts.clear();
    phase.verdict_routes.clear();
    phase.followup_contracts.clear();
    phase.skills.clear();
    phase.resources.clear();
    phase.fresh_reviewer_input = false;

    let mut request = CompletePipelinePhase {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: phase.id.clone(),
        outcome: PipelinePhaseOutcome::Completed,
        transition: PipelineTransition::Continue,
        output: PipelinePhaseOutputDraft {
            body: "semantic v0.7 output".into(),
            producer_context_id: "agent-context".into(),
            fields: BTreeMap::new(),
            verdict: None,
            dispositions: Vec::new(),
            skill_reads: Vec::new(),
            resource_reads: Vec::new(),
            artifacts: Vec::new(),
            evidence_artifacts: Vec::new(),
            validator_receipts: Vec::new(),
            followup_proposal: None,
            reviewer_context: None,
            reference: None,
            knowledge_publication: None,
        },
        consumed_outputs: Vec::new(),
        consumed_inputs: Vec::new(),
        revisit_phase_id: None,
        escalation_target: None,
        terminal_result: None,
        publish_blocked_result: false,
        consumed_knowledge: None,
        research_checkpoint: None,
    };
    assert!(request.validate(&v07).is_ok());

    let mut omitted = serde_json::to_value(&request).unwrap();
    omitted.as_object_mut().unwrap().remove("consumed_outputs");
    omitted.as_object_mut().unwrap().remove("consumed_inputs");
    let omitted: CompletePipelinePhase = serde_json::from_value(omitted).unwrap();
    assert!(omitted.consumed_outputs.is_empty());
    assert!(omitted.consumed_inputs.is_empty());
    assert!(omitted.validate(&v07).is_ok());

    let proof_paths = [
        "arguments.params.consumed_outputs",
        "arguments.params.consumed_inputs",
        "arguments.params.consumed_knowledge",
        "arguments.params.output.skill_reads",
        "arguments.params.output.resource_reads",
    ];
    for path in proof_paths {
        match path {
            "arguments.params.consumed_outputs" => {
                request.consumed_outputs = vec![PipelineConsumedOutput {
                    phase_id: "prior".into(),
                    output_revision: 1,
                    digest: "digest".into(),
                }];
            }
            "arguments.params.consumed_inputs" => {
                request.consumed_outputs.clear();
                request.consumed_inputs = vec![PipelineConsumedInput {
                    input_id: Uuid::new_v4(),
                    sequence: 1,
                    digest: "digest".into(),
                }];
            }
            "arguments.params.consumed_knowledge" => {
                request.consumed_outputs.clear();
                request.consumed_inputs.clear();
                request.consumed_knowledge = Some(ConsumedKnowledgeManifestRef {
                    manifest_id: Uuid::new_v4(),
                    digest: "manifest-digest".into(),
                });
            }
            "arguments.params.output.skill_reads" => {
                request.consumed_outputs.clear();
                request.consumed_inputs.clear();
                request.consumed_knowledge = None;
                request.output.skill_reads = vec![PipelineSkillReadReceipt {
                    instruction_id: "skill".into(),
                    version: "1".into(),
                    digest: "digest".into(),
                }];
            }
            "arguments.params.output.resource_reads" => {
                request.consumed_outputs.clear();
                request.consumed_inputs.clear();
                request.consumed_knowledge = None;
                request.output.skill_reads.clear();
                request.output.resource_reads = vec![PipelineSkillReadReceipt {
                    instruction_id: "resource".into(),
                    version: "1".into(),
                    digest: "digest".into(),
                }];
            }
            _ => unreachable!(),
        }
        let error = request.validate(&v07).unwrap_err();
        assert_eq!(error.code(), "BACKEND_DERIVED_PROOF_REQUIRED");
        let refusal = error.refusal().unwrap();
        assert_eq!(refusal.rule.as_deref(), Some("WP3-PROOF-01"));
        assert_eq!(refusal.path.as_deref(), Some(path));
        assert_eq!(
            refusal.expected.as_deref(),
            Some("omitted; backend derives the proof")
        );
        assert_eq!(refusal.actual.as_deref(), Some("agent-supplied value"));
        request.consumed_outputs.clear();
        request.consumed_inputs.clear();
        request.consumed_knowledge = None;
        request.output.skill_reads.clear();
        request.output.resource_reads.clear();
    }

    let legacy = load(
        include_str!("../../../pipeline-definitions/lightweight-tdd-0.1.0-native.1.json"),
        PipelineKind::LightweightTddDevelopment,
    )
    .unwrap();
    let phase = legacy.phases[0].clone();
    let legacy_request = CompletePipelinePhase {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: phase.id.clone(),
        outcome: PipelinePhaseOutcome::Completed,
        transition: PipelineTransition::Continue,
        output: PipelinePhaseOutputDraft {
            body: "legacy output".into(),
            producer_context_id: "legacy-context".into(),
            fields: BTreeMap::new(),
            verdict: None,
            dispositions: Vec::new(),
            skill_reads: phase
                .skills
                .iter()
                .map(|value| PipelineSkillReadReceipt {
                    instruction_id: value.id.clone(),
                    version: value.version.clone(),
                    digest: value.digest.clone(),
                })
                .collect(),
            resource_reads: phase
                .resources
                .iter()
                .map(|value| PipelineSkillReadReceipt {
                    instruction_id: value.id.clone(),
                    version: value.version.clone(),
                    digest: value.digest.clone(),
                })
                .collect(),
            artifacts: Vec::new(),
            evidence_artifacts: Vec::new(),
            validator_receipts: Vec::new(),
            followup_proposal: None,
            reviewer_context: None,
            reference: None,
            knowledge_publication: None,
        },
        consumed_outputs: Vec::new(),
        consumed_inputs: Vec::new(),
        revisit_phase_id: None,
        escalation_target: None,
        terminal_result: None,
        publish_blocked_result: false,
        consumed_knowledge: None,
        research_checkpoint: None,
    };
    let mut legacy_request = legacy_request;
    legacy_request.consumed_outputs = vec![PipelineConsumedOutput {
        phase_id: "legacy-prior".into(),
        output_revision: 2,
        digest: "legacy-output-digest".into(),
    }];
    legacy_request.consumed_inputs = vec![PipelineConsumedInput {
        input_id: Uuid::new_v4(),
        sequence: 3,
        digest: "legacy-input-digest".into(),
    }];
    legacy_request.consumed_knowledge = Some(ConsumedKnowledgeManifestRef {
        manifest_id: Uuid::new_v4(),
        digest: "legacy-manifest-digest".into(),
    });
    let decoded: CompletePipelinePhase =
        serde_json::from_value(serde_json::to_value(&legacy_request).unwrap()).unwrap();
    assert_eq!(
        decoded.output.skill_reads,
        legacy_request.output.skill_reads
    );
    assert_eq!(
        decoded.output.resource_reads,
        legacy_request.output.resource_reads
    );
    assert_eq!(decoded.consumed_outputs, legacy_request.consumed_outputs);
    assert_eq!(decoded.consumed_inputs, legacy_request.consumed_inputs);
    assert!(legacy.version.starts_with("0.1"));
}
