use super::*;

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
        include_str!("../../../pipeline-definitions/research-0.5.0-native.inquiry.1.json"),
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
