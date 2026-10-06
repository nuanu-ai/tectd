use super::*;

#[test]
fn full_definition_is_phasewise_and_retains_resources_and_artifacts() {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::FullDesignToExecution)
        .unwrap();
    assert_eq!(definition.phases.len(), 21);
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
    assert!(
        definition.phases.iter().any(|phase| phase.retry_policy
            == tect_domain::PipelinePhaseRetryPolicy::ReconciliationRequired)
    );
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
    assert_eq!(definition.version, "0.4.0-native.skills.2");
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
    assert_eq!(definition.version, "0.4.0-native.skills.2");
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
                "../../../pipeline-definitions/research-to-durable-knowledge-0.2.0-native.dk2.1.json"
            ),
            include_str!(
                "../../../pipeline-definitions/research-to-durable-knowledge-0.1.0-native.1.json"
            ),
            [
                "slice-research-promotion-gate",
                "slice-research-index-front-door-checker",
                "slice-research-result-and-handoff-writer",
            ],
        ),
        (
            include_str!("../../../pipeline-definitions/procedure-capture-0.2.0-native.dk2.1.json"),
            include_str!("../../../pipeline-definitions/procedure-capture-0.1.0-native.1.json"),
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
                !bodies
                    .iter()
                    .any(|body| body.id
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
    assert_eq!(phases, 117);
}
