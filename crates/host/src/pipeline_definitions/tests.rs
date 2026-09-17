use super::*;

#[test]
fn lightweight_definition_has_exact_complete_bodies() {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::LightweightTddDevelopment)
        .unwrap();
    assert_eq!(definition.phases.len(), 15);
    assert!(definition.phases.iter().all(|phase| {
        !phase.instructions.is_empty()
            && (phase.id == "slice-lightweight-pre-implementation-review"
                || phase.instructions.iter().all(|body| body.body.len() > 1000))
    }));
    assert_eq!(definition.phases[3].skills.len(), 1);
    assert!(!definition.phases[8].skills.is_empty());
    assert_eq!(definition.phases[10].skills.len(), 1);
}

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
                "../../pipeline-definitions/research-to-durable-knowledge-0.2.0-native.dk2.1.json"
            ),
            include_str!(
                "../../pipeline-definitions/research-to-durable-knowledge-0.1.0-native.1.json"
            ),
            [
                "slice-research-promotion-gate",
                "slice-research-index-front-door-checker",
                "slice-research-result-and-handoff-writer",
            ],
        ),
        (
            include_str!("../../pipeline-definitions/procedure-capture-0.2.0-native.dk2.1.json"),
            include_str!("../../pipeline-definitions/procedure-capture-0.1.0-native.1.json"),
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
    assert_eq!(phases, 127);
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
            research_checkpoint: None,
        };
        definition.phases = vec![phase];
        (definition, completion)
    }

    let old = load(
        include_str!("../../pipeline-definitions/lightweight-tdd-0.1.0-native.1.json"),
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
fn inquiry_definitions_package_exact_ordered_methods_and_delivery_modes() {
    for (kind, expected_version, expected_phases, terminal) in [
        (PipelineKind::Research, "0.5.1-native.inquiry.2", 12, "R12"),
        (
            PipelineKind::DeepBrainstorming,
            "0.5.0-native.inquiry.1",
            10,
            "B10",
        ),
    ] {
        let definition = StaticPipelineDefinitions.definition(kind).unwrap();
        assert_eq!(definition.version, expected_version);
        assert_eq!(definition.phases.len(), expected_phases);
        assert_eq!(
            definition.default_mode,
            tect_domain::PipelineDeliveryMode::Phasewise
        );
        assert_eq!(
            definition.allowed_modes,
            vec![
                tect_domain::PipelineDeliveryMode::Phasewise,
                tect_domain::PipelineDeliveryMode::Whole,
            ]
        );
        assert_eq!(definition.phases.last().unwrap().id, terminal);
        assert!(definition.phases.iter().all(|phase| {
            phase.required
                && !phase.disposition_required
                && !phase.instructions.is_empty()
                && phase
                    .instructions
                    .iter()
                    .all(|body| body.id == "tect:inquiry-boundary")
                && !phase.skills.is_empty()
                && !phase.required_artifacts.is_empty()
                && !phase.verdict_routes.is_empty()
                && !phase.fresh_reviewer_input
                && phase.retry_policy == tect_domain::PipelinePhaseRetryPolicy::Repeatable
        }));
    }
}

#[test]
fn inquiry_definition_special_reads_and_terminal_routes_are_exact() {
    let research = StaticPipelineDefinitions
        .definition(PipelineKind::Research)
        .unwrap();
    let r03 = &research.phases[2];
    assert!(
        r03.skills
            .iter()
            .any(|body| body.id == "superpowers:writing-plans")
    );
    assert_eq!(r03.resources.len(), 2);
    let r11 = &research.phases[10];
    assert!(
        r11.skills
            .iter()
            .any(|body| body.id == "superpowers:verification-before-completion")
    );
    assert_eq!(r11.resources[0].id, "tect:superpowers-v6-native-boundary");
    let r12 = &research.phases[11];
    assert!(
        r12.verdict_routes
            .iter()
            .filter(|route| route.transition == tect_domain::PipelineTransition::Complete)
            .all(|route| matches!(
                route.verdict.as_str(),
                "answered" | "negative_result" | "inconclusive"
            ))
    );

    let brainstorming = StaticPipelineDefinitions
        .definition(PipelineKind::DeepBrainstorming)
        .unwrap();
    let b05 = &brainstorming.phases[4];
    assert!(b05.required_artifacts.iter().any(|artifact| {
        artifact.name_pattern == "evidence-checkpoint.md"
            && artifact.when_verdict.as_deref() == Some("waiting_research")
    }));
    let b08 = &brainstorming.phases[7];
    assert!(b08.required_artifacts.iter().any(|artifact| {
        artifact.name_pattern == "decision-disposition.md"
            && artifact.media_type == "text/markdown"
            && artifact.required
            && artifact.when_verdict.as_deref() == Some("pending_decision")
    }));
    let b10 = &brainstorming.phases[9];
    assert!(
        b10.skills
            .iter()
            .any(|body| body.id == "superpowers:verification-before-completion")
    );
    assert_eq!(b10.resources[0].id, "tect:superpowers-v6-native-boundary");
}

#[test]
fn research_contract_derives_all_phases_classifications_provenance_and_publication_boundary() {
    use tect_domain::PipelineOutputConstraint;

    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::Research)
        .unwrap();
    assert_eq!(
        definition
            .phases
            .iter()
            .map(|phase| phase.id.as_str())
            .collect::<Vec<_>>(),
        (1..=12)
            .map(|ordinal| format!("R{ordinal:02}"))
            .collect::<Vec<_>>()
    );
    assert!(definition.phases.iter().all(|phase| {
        phase.required
            && !phase.instructions.is_empty()
            && !phase.skills.is_empty()
            && phase.output_constraints.iter().all(|constraint| {
                !matches!(
                    constraint,
                    PipelineOutputConstraint::ResolvedKnowledgePublication { .. }
                        | PipelineOutputConstraint::EngineeringReview { .. }
                        | PipelineOutputConstraint::CodeAuthorization { .. }
                )
            })
    }));
    let r09 = &definition.phases[8];
    assert_eq!(r09.id, "R09");
    let sufficiency = &r09.skills[0].body;
    assert!(sufficiency.contains("every required material target is supported or resolved"));
    assert!(sufficiency.contains(
        "classifying a required target as unresolved does not make the overall result ready"
    ));
    assert!(sufficiency.contains("specific authorized bounded read has useful information gain"));
    assert!(sufficiency.contains("At the terminal decision"));
    assert!(sufficiency.contains("immutable `allow_inconclusive` flag is true"));
    for (verdict, outcome) in [
        ("ready", tect_domain::PipelinePhaseOutcome::Completed),
        (
            "bounded_inconclusive",
            tect_domain::PipelinePhaseOutcome::Completed,
        ),
        (
            "waiting_source",
            tect_domain::PipelinePhaseOutcome::WaitingInput,
        ),
    ] {
        assert!(
            r09.verdict_routes
                .iter()
                .any(|route| { route.verdict == verdict && route.outcome == outcome })
        );
    }
    let r12 = &definition.phases[11];
    assert!(
        r12.skills[0]
            .body
            .contains("use the `inconclusive` verdict and `result_state=inconclusive`")
    );
    for verdict in ["answered", "negative_result", "inconclusive"] {
        assert!(r12.verdict_routes.iter().any(|route| {
            route.verdict == verdict
                && route.outcome == tect_domain::PipelinePhaseOutcome::Completed
                && route.transition == tect_domain::PipelineTransition::Complete
        }));
        assert!(r12.output_constraints.iter().any(|constraint| matches!(
            constraint,
            PipelineOutputConstraint::FieldEquals { field, value, when_verdict }
                if field == "publication_status"
                    && value == "not_performed"
                    && when_verdict.as_deref() == Some(verdict)
        )));
    }
    for phase_id in ["R06", "R07", "R08", "R09", "R11", "R12"] {
        let phase = definition
            .phases
            .iter()
            .find(|phase| phase.id == phase_id)
            .unwrap();
        assert!(
            phase
                .output_contract
                .to_ascii_lowercase()
                .contains("source")
                || phase
                    .output_contract
                    .to_ascii_lowercase()
                    .contains("evidence")
                || phase
                    .output_contract
                    .to_ascii_lowercase()
                    .contains("provenance"),
            "{phase_id} must preserve evidence provenance"
        );
    }
}

#[test]
fn archived_research_inquiry_snapshot_preserves_initial_definition() {
    let archived = load(
        include_str!("../../pipeline-definitions/research-0.5.0-native.inquiry.1.json"),
        PipelineKind::Research,
    )
    .unwrap();
    assert_eq!(archived.version, "0.5.0-native.inquiry.1");
    assert_eq!(
        archived.digest,
        "d3425b463b589897cc4c66157fa8d1bfc05ef200f7593ca46e7e4f566073e612"
    );
}

#[test]
fn brainstorming_contract_derives_all_phases_and_exact_b05_research_checkpoint() {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::DeepBrainstorming)
        .unwrap();
    assert_eq!(
        definition
            .phases
            .iter()
            .map(|phase| phase.id.as_str())
            .collect::<Vec<_>>(),
        (1..=10)
            .map(|ordinal| format!("B{ordinal:02}"))
            .collect::<Vec<_>>()
    );
    let b05 = &definition.phases[4];
    assert!(b05.verdict_routes.iter().any(|route| {
        route.verdict == "waiting_research"
            && route.outcome == tect_domain::PipelinePhaseOutcome::WaitingInput
            && route.transition == tect_domain::PipelineTransition::Continue
    }));
    assert!(b05.required_artifacts.iter().any(|artifact| {
        artifact.name_pattern == "evidence-checkpoint.md"
            && artifact.when_verdict.as_deref() == Some("waiting_research")
    }));
    assert!(b05.output_contract.contains("exact typed checkpoint"));
    assert!(b05.output_contract.contains("returned Research result"));
    assert!(definition.overview.body.contains("B05"));
    assert!(definition.overview.body.contains("accepted exact result"));
    assert!(
        b05.skills[0]
            .body
            .contains("exact result from the bound Research")
    );
    assert!(b05.skills[0].body.contains("On resume"));
}

#[test]
fn frozen_v04_pipeline_definitions_preserve_bytes_digest_parse_and_phase_identity() {
    let fixtures = [
        (
            include_str!("../../pipeline-definitions/lightweight-tdd-0.4.0-native.skills.1.json"),
            PipelineKind::LightweightTddDevelopment,
            "66983d90c2fc8f17f91cec02a29dcd3bc382c2967f683a7392dc1378561fb921",
            "b80b3472ebf4acc38996fa1946a2fe76e1b17fbcc39c6594f87a00e63a437768",
            &[
                "slice-lightweight-entry-gate",
                "slice-lightweight-intent-capture",
                "slice-lightweight-context-loader",
                "slice-workspace-preflight-lite",
                "slice-lightweight-contract-writer",
                "slice-lightweight-escalation-checker",
                "slice-test-target-selector",
                "slice-tdd-cycle-runner",
                "slice-implementation-note-writer",
                "slice-lightweight-verification-runner",
                "slice-deploy-impact-checker",
                "slice-lightweight-result-writer",
                "slice-lightweight-promotion-router",
                "slice-lightweight-maintenance-and-handoff",
            ][..],
        ),
        (
            include_str!(
                "../../pipeline-definitions/full-design-to-execution-0.4.0-native.skills.1.json"
            ),
            PipelineKind::FullDesignToExecution,
            "eb36e20697b38204a5a10261f5454e854538b6c719213f9d9b6be0303663bab1",
            "13fd152337abc76d7bbfa15c0875d7b6fbe4719ccfd6fadab5f31825cd39769b",
            &[
                "slice-full-dev-entry-gate",
                "slice-workspace-preflight",
                "slice-design-spec-shaper",
                "slice-contract-writer",
                "slice-component-decision-interrogator",
                "slice-cross-cutting-reviewer",
                "slice-reconciliation-runner",
                "slice-implementation-spec-synthesizer",
                "slice-spec-readiness-checker",
                "slice-plan-builder",
                "slice-human-decision-queue-manager",
                "slice-execution-runner",
                "slice-verification-runner",
                "slice-validation-deployment-contract-shaper",
                "slice-deployment-or-handoff-gate",
                "slice-live-validation-runner",
                "slice-result-writer",
                "slice-promotion-and-deferred-router",
                "slice-maintenance-check-requester",
                "slice-handoff-builder",
            ][..],
        ),
    ];

    for (source, kind, file_sha256, definition_digest, phases) in fixtures {
        assert_eq!(hex(&Sha256::digest(source.as_bytes())), file_sha256);
        let parsed: PipelineDefinitionSnapshot = serde_json::from_str(source).unwrap();
        assert_eq!(parsed.kind, kind);
        assert_eq!(parsed.version, "0.4.0-native.skills.1");
        assert_eq!(parsed.digest, definition_digest);
        assert_eq!(
            parsed
                .phases
                .iter()
                .map(|phase| phase.id.as_str())
                .collect::<Vec<_>>(),
            phases
        );
        let loaded = load(source, kind).unwrap();
        assert_eq!(loaded, parsed);
    }
}

#[test]
fn non_coding_pipeline_definitions_expose_no_engineering_or_code_authority() {
    use tect_domain::PipelineOutputConstraint;

    for kind in [
        PipelineKind::DebugRootCause,
        PipelineKind::OperationalPreparation,
        PipelineKind::OperationalExecution,
        PipelineKind::Research,
        PipelineKind::DeepBrainstorming,
        PipelineKind::ResearchToDurableKnowledge,
        PipelineKind::CustomProcedureCapture,
    ] {
        let definition = StaticPipelineDefinitions.definition(kind).unwrap();
        assert!(
            definition.phases.iter().all(|phase| {
                phase.output_constraints.iter().all(|constraint| {
                    !matches!(
                        constraint,
                        PipelineOutputConstraint::EngineeringReview { .. }
                            | PipelineOutputConstraint::CodeAuthorization { .. }
                    )
                })
            }),
            "{} exposed engineering or code authority",
            kind.as_str()
        );
    }
}

#[test]
fn non_coding_pipeline_definitions_reject_forged_engineering_authority_constraints() {
    use tect_domain::PipelineOutputConstraint;

    for kind in [
        PipelineKind::DebugRootCause,
        PipelineKind::OperationalPreparation,
        PipelineKind::OperationalExecution,
        PipelineKind::Research,
        PipelineKind::DeepBrainstorming,
        PipelineKind::ResearchToDurableKnowledge,
        PipelineKind::CustomProcedureCapture,
    ] {
        let mut definition = StaticPipelineDefinitions.definition(kind).unwrap();
        definition.phases[0]
            .output_constraints
            .push(PipelineOutputConstraint::CodeAuthorization {
                required_plan_review_phase_id: "forged-engineering-review".into(),
            });
        assert!(
            definition.validate().is_err(),
            "{} accepted forged code authority",
            kind.as_str()
        );

        let mut definition = StaticPipelineDefinitions.definition(kind).unwrap();
        let forged_success_verdict = definition.phases[0].allowed_verdicts[0].clone();
        definition.phases[0]
            .output_constraints
            .push(PipelineOutputConstraint::EngineeringReview {
                stage: "plan".into(),
                standards_resource_id: "tect:engineering-standards".into(),
                standards_resource_digest: "forged".into(),
                artifact_name: "engineering-review.json".into(),
                success_verdicts: vec![forged_success_verdict],
                required_prior_review_phase_ids: vec![],
                required_reconciliation_phase_id: None,
            });
        assert!(
            definition.validate().is_err(),
            "{} accepted forged review authority",
            kind.as_str()
        );
    }
}
