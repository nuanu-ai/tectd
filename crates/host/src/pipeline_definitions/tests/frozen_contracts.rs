use super::*;

#[test]
fn frozen_v04_pipeline_definitions_preserve_bytes_digest_parse_and_phase_identity() {
    let fixtures = [
        (
            include_str!("fixtures/lightweight-tdd-0.4.0-native.skills.1.json"),
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
                "../../../pipeline-definitions/full-design-to-execution-0.4.0-native.skills.1.json"
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
