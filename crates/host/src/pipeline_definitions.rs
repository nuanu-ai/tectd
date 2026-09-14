use sha2::{Digest, Sha256};
use tect_application::PipelineDefinitionProvider;
use tect_domain::{Error, PipelineDefinitionSnapshot, PipelineKind, Result};

pub(crate) struct StaticPipelineDefinitions;

impl PipelineDefinitionProvider for StaticPipelineDefinitions {
    fn definition(&self, kind: PipelineKind) -> Result<PipelineDefinitionSnapshot> {
        match kind {
            PipelineKind::LightweightTddDevelopment => load(
                include_str!("../pipeline-definitions/lightweight-tdd.json"),
                kind,
            ),
            PipelineKind::FullDesignToExecution => load(
                include_str!("../pipeline-definitions/full-design-to-execution.json"),
                kind,
            ),
            PipelineKind::DebugRootCause => load(
                include_str!("../pipeline-definitions/debug-root-cause.json"),
                kind,
            ),
            PipelineKind::OperationalPreparation => load(
                include_str!("../pipeline-definitions/operational-preparation.json"),
                kind,
            ),
            PipelineKind::OperationalExecution => load(
                include_str!("../pipeline-definitions/operational-execution.json"),
                kind,
            ),
            PipelineKind::ResearchToDurableKnowledge => load(
                include_str!("../pipeline-definitions/research-to-durable-knowledge.json"),
                kind,
            ),
            PipelineKind::CustomProcedureCapture => load(
                include_str!("../pipeline-definitions/procedure-capture.json"),
                kind,
            ),
            PipelineKind::PromoteToDurableKnowledge => Err(Error::KnowledgeLifecycleRequired),
        }
    }
}

pub(crate) fn delivery_modes(
    kind: PipelineKind,
) -> Option<(
    tect_domain::PipelineDeliveryMode,
    Vec<tect_domain::PipelineDeliveryMode>,
)> {
    StaticPipelineDefinitions
        .definition(kind)
        .ok()
        .map(|definition| (definition.default_mode, definition.allowed_modes))
}

fn load(source: &str, expected: PipelineKind) -> Result<PipelineDefinitionSnapshot> {
    let definition: PipelineDefinitionSnapshot =
        serde_json::from_str(source).map_err(|_| Error::InvalidConfiguration)?;
    if definition.kind != expected {
        return Err(Error::InvalidConfiguration);
    }
    let expected_digest = definition.digest.clone();
    let mut material = definition.clone();
    material.digest.clear();
    let bytes = serde_json::to_vec(&material).map_err(|_| Error::InvalidConfiguration)?;
    if hex(&Sha256::digest(bytes)) != expected_digest {
        return Err(Error::InvalidConfiguration);
    }
    for body in
        std::iter::once(&definition.overview).chain(definition.phases.iter().flat_map(|phase| {
            phase
                .instructions
                .iter()
                .chain(&phase.skills)
                .chain(&phase.resources)
        }))
    {
        if hex(&Sha256::digest(body.body.as_bytes())) != body.digest {
            return Err(Error::InvalidConfiguration);
        }
    }
    definition
        .validate()
        .map_err(|_| Error::InvalidConfiguration)?;
    Ok(definition)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lightweight_definition_has_exact_complete_bodies() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::LightweightTddDevelopment)
            .unwrap();
        assert_eq!(definition.phases.len(), 14);
        assert!(definition.phases.iter().all(|phase| {
            !phase.instructions.is_empty()
                && phase.instructions.iter().all(|body| body.body.len() > 1000)
        }));
        assert_eq!(definition.phases[3].skills.len(), 1);
        assert!(!definition.phases[7].skills.is_empty());
        assert_eq!(definition.phases[9].skills.len(), 1);
    }

    #[test]
    fn full_definition_is_phasewise_and_retains_resources_and_artifacts() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::FullDesignToExecution)
            .unwrap();
        assert_eq!(definition.phases.len(), 20);
        assert_eq!(definition.allowed_modes.len(), 1);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Phasewise
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.resources.is_empty())
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
        assert_eq!(
            definition
                .phases
                .iter()
                .map(|phase| phase.validator_contracts.len())
                .sum::<usize>(),
            2
        );
    }

    #[test]
    fn debug_definition_is_complete_and_allows_both_delivery_modes() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::DebugRootCause)
            .unwrap();
        assert_eq!(definition.phases.len(), 18);
        assert_eq!(definition.allowed_modes.len(), 2);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Whole
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
    }

    #[test]
    fn operational_preparation_is_complete_and_allows_both_delivery_modes() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::OperationalPreparation)
            .unwrap();
        assert_eq!(definition.phases.len(), 16);
        assert_eq!(definition.allowed_modes.len(), 2);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Whole
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
    }

    #[test]
    fn operational_execution_is_complete_and_phasewise_only() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::OperationalExecution)
            .unwrap();
        assert_eq!(definition.phases.len(), 18);
        assert_eq!(definition.allowed_modes.len(), 1);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Phasewise
        );
        assert!(definition.phases.iter().any(|phase| phase.retry_policy
            == tect_domain::PipelinePhaseRetryPolicy::ReconciliationRequired));
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
    }

    #[test]
    fn research_definition_is_complete_and_defaults_to_phasewise() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::ResearchToDurableKnowledge)
            .unwrap();
        assert_eq!(definition.phases.len(), 22);
        assert_eq!(definition.version, "0.4.0-native.skills.1");
        assert_eq!(definition.allowed_modes.len(), 2);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Phasewise
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.allowed_backward_to.is_empty())
        );
    }

    #[test]
    fn procedure_capture_definition_is_complete_and_defaults_to_whole() {
        let definition = StaticPipelineDefinitions
            .definition(PipelineKind::CustomProcedureCapture)
            .unwrap();
        assert_eq!(definition.phases.len(), 17);
        assert_eq!(definition.version, "0.4.0-native.skills.1");
        assert_eq!(definition.allowed_modes.len(), 2);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Whole
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.required_artifacts.is_empty())
        );
        assert!(
            definition
                .phases
                .iter()
                .any(|phase| !phase.skills.is_empty())
        );
    }

    #[test]
    fn dk2_producer_versions_preserve_phase_bodies_and_pin_handoff_resource() {
        for (current, archived, expected_phases) in [
            (
                include_str!(
                    "../pipeline-definitions/research-to-durable-knowledge-0.2.0-native.dk2.1.json"
                ),
                include_str!(
                    "../pipeline-definitions/research-to-durable-knowledge-0.1.0-native.1.json"
                ),
                [
                    "slice-research-promotion-gate",
                    "slice-research-index-front-door-checker",
                    "slice-research-result-and-handoff-writer",
                ],
            ),
            (
                include_str!("../pipeline-definitions/procedure-capture-0.2.0-native.dk2.1.json"),
                include_str!("../pipeline-definitions/procedure-capture-0.1.0-native.1.json"),
                [
                    "slice-procedure-promotion-gate",
                    "slice-procedure-result-writer",
                    "slice-procedure-maintenance-and-handoff",
                ],
            ),
        ] {
            let new: serde_json::Value = serde_json::from_str(current).unwrap();
            let old: serde_json::Value = serde_json::from_str(archived).unwrap();
            assert_eq!(old["version"], "0.1.0-native.1");
            assert_eq!(new["version"], "0.2.0-native.dk2.1");
            let new_phases = new["phases"].as_array().unwrap();
            let old_phases = old["phases"].as_array().unwrap();
            assert_eq!(new_phases.len(), old_phases.len());
            for (new_phase, old_phase) in new_phases.iter().zip(old_phases) {
                assert_eq!(new_phase["id"], old_phase["id"]);
                assert_eq!(new_phase["instructions"], old_phase["instructions"]);
                assert_eq!(new_phase["skills"], old_phase["skills"]);
                let old_resources = old_phase["resources"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let new_resources = new_phase["resources"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                assert_eq!(&new_resources[..old_resources.len()], old_resources);
                let expected = expected_phases.contains(&new_phase["id"].as_str().unwrap());
                assert_eq!(
                    new_resources.len(),
                    old_resources.len() + usize::from(expected)
                );
                if expected {
                    let method = new_resources.last().unwrap();
                    assert_eq!(
                        method["id"],
                        "tect:knowledge-change:producer-publication-handoff"
                    );
                    assert_eq!(method["version"], "0.2.0-dk2.1");
                    assert_eq!(
                        method["digest"],
                        hex(&Sha256::digest(method["body"].as_str().unwrap().as_bytes()))
                    );
                }
            }
            let guarded = new_phases
                .iter()
                .filter(|phase| {
                    phase["output_constraints"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|constraint| constraint["kind"] == "resolved_knowledge_publication")
                })
                .collect::<Vec<_>>();
            assert_eq!(guarded.len(), 1);
        }
    }

    #[test]
    fn selected_superpowers_v6_bodies_require_native_category_adapters() {
        let kinds = [
            PipelineKind::LightweightTddDevelopment,
            PipelineKind::FullDesignToExecution,
            PipelineKind::DebugRootCause,
            PipelineKind::OperationalPreparation,
            PipelineKind::OperationalExecution,
            PipelineKind::ResearchToDurableKnowledge,
            PipelineKind::CustomProcedureCapture,
        ];
        let selected = [
            "superpowers:test-driven-development",
            "superpowers:test-driven-development/writing-good-tests",
            "superpowers:using-git-worktrees",
            "superpowers:finishing-a-development-branch",
            "superpowers:executing-plans",
            "superpowers:requesting-code-review",
            "superpowers:requesting-code-review/code-reviewer",
            "superpowers:writing-plans",
            "superpowers:systematic-debugging",
        ];
        let mut phases = 0;
        for kind in kinds {
            let definition = StaticPipelineDefinitions.definition(kind).unwrap();
            assert_eq!(definition.version, "0.4.0-native.skills.1");
            phases += definition.phases.len();
            for phase in &definition.phases {
                let bodies = phase
                    .skills
                    .iter()
                    .chain(&phase.resources)
                    .collect::<Vec<_>>();
                let uses_selected = bodies
                    .iter()
                    .any(|body| selected.contains(&body.id.as_str()));
                let adapters = phase
                    .resources
                    .iter()
                    .filter(|body| body.id.starts_with("tect:superpowers-v6-"))
                    .collect::<Vec<_>>();
                assert_eq!(uses_selected, !adapters.is_empty(), "{}", phase.id);
                if uses_selected {
                    assert!(
                        adapters
                            .iter()
                            .any(|body| body.id == "tect:superpowers-v6-native-boundary")
                    );
                    assert!(
                        adapters
                            .iter()
                            .all(|body| body.version == "0.4.0-native.skills.1")
                    );
                }
                assert!(
                    !bodies.iter().any(|body| body.id
                        == "superpowers:test-driven-development/testing-anti-patterns")
                );
                assert!(
                    bodies
                        .iter()
                        .filter(|body| body.id.starts_with("tect:superpowers-v6-"))
                        .map(|body| &body.id)
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        == adapters.len()
                );
            }
        }
        assert_eq!(phases, 125);
    }

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
            };
            definition.phases = vec![phase];
            (definition, completion)
        }

        let old = load(
            include_str!("../pipeline-definitions/lightweight-tdd-0.1.0-native.1.json"),
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
}
