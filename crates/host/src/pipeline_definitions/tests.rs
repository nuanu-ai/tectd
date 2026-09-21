use super::*;

#[test]
fn embedded_definition_inventory_is_exact() {
    let provider = StaticPipelineDefinitions;
    for (kind, version, digest) in [
        (
            PipelineKind::LightweightTddDevelopment,
            "0.6.0-native.engineering.2",
            "bef9f376f187b985684005b075275a072625f1c08125991062bb38c47ac884b1",
        ),
        (
            PipelineKind::FullDesignToExecution,
            "0.6.0-native.engineering.2",
            "1274c531dfd433bf01e6b2354adcd0082c906749e1c8e34a158604f77e77a9a5",
        ),
        (
            PipelineKind::DebugRootCause,
            "0.4.0-native.skills.2",
            "afb0f21932a11eceb8e3aba01d3d07ec9f74160203085391f7de1758118a6574",
        ),
        (
            PipelineKind::OperationalPreparation,
            "0.4.0-native.skills.2",
            "6dcf48ec7712fcc3a9dc1f40c83c2337313bfadb33cbfdbe5d76b71da455d2b4",
        ),
        (
            PipelineKind::OperationalExecution,
            "0.4.0-native.skills.2",
            "47046a703413f6e3048c6923b87dae6ceb0bbecb3c9e0f9d60ca614562267e79",
        ),
        (
            PipelineKind::ResearchToDurableKnowledge,
            "0.4.0-native.skills.2",
            "2bd0c1c0d9403066936d7c73f44dd139a3829a31efe16fe0e653cd1dcf6c4631",
        ),
        (
            PipelineKind::Research,
            "0.5.1-native.inquiry.2",
            "7d9a817dbbd4560aca33f46522027cf2aefad483bf5837bb98b494d533f115af",
        ),
        (
            PipelineKind::DeepBrainstorming,
            "0.5.0-native.inquiry.1",
            "2b7071f75bd5c9d61815c443b550ab452f7eeff3490e9fe7fe3dd71fc90d3227",
        ),
        (
            PipelineKind::CustomProcedureCapture,
            "0.4.0-native.skills.2",
            "1e439fa7521bcd607949ae7856672f2e718320b383c7af2bfb61cc9a39481a2f",
        ),
    ] {
        let definition = provider.definition(kind).unwrap();
        assert_eq!(definition.version, version, "{}", kind.as_str());
        assert_eq!(definition.digest, digest, "{}", kind.as_str());
    }

    for (version, digest) in [
        (
            "0.7.0-native.k1k5",
            "7f5dd6a4503078538d45d0c90c83fdcd896ff1216167556ff9bd0424f826aab0",
        ),
        (
            "0.7.1-native.k1k5",
            "93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89",
        ),
    ] {
        let definition = provider
            .definition_for(PipelineKind::LightweightTddDevelopment, Some(version))
            .unwrap();
        assert_eq!(definition.version, version);
        assert_eq!(definition.digest, digest);
    }
}

#[test]
fn lightweight_v07_is_immutable_compact_and_traceable() {
    let definition = lightweight_v07().expect("v0.7 definition loads");
    definition.validate().expect("v0.7 definition validates");
    assert_eq!(definition.version, "0.7.1-native.k1k5");
    assert_eq!(
        definition.digest,
        "93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89"
    );
    assert_eq!(definition.phases.len(), 5);
    assert_eq!(
        definition
            .phases
            .iter()
            .map(|p| p.required_fields.len())
            .max(),
        Some(8)
    );
    assert!(definition.phases.iter().all(|phase| {
        phase
            .instructions
            .iter()
            .all(|body| body.body.len() <= 4096)
    }));
    assert!(definition.overview.origin_refs.len() >= 15);
}

#[test]
fn lightweight_v07_routes_cover_pass_rework_block_and_escalation() {
    let definition = lightweight_v07().unwrap();
    for phase in &definition.phases {
        assert_eq!(phase.verdict_routes.len(), 4);
        assert!(
            phase
                .verdict_routes
                .iter()
                .any(|route| route.verdict == "pass")
        );
        assert!(
            phase
                .verdict_routes
                .iter()
                .any(|route| route.verdict == "rework")
        );
        assert!(
            phase
                .verdict_routes
                .iter()
                .any(|route| route.verdict == "blocked")
        );
        assert!(
            phase
                .verdict_routes
                .iter()
                .any(|route| route.verdict == "escalate")
        );
    }
}

fn v07_completion(
    phase_id: &str,
) -> (
    PipelineDefinitionSnapshot,
    tect_domain::CompletePipelinePhase,
) {
    use std::collections::BTreeMap;
    use tect_domain::{
        CompletePipelinePhase, PipelinePhaseOutcome, PipelinePhaseOutputDraft,
        PipelineTerminalResultDraft, PipelineTransition, SliceResultEvidence,
    };
    use uuid::Uuid;

    let definition = lightweight_v07().unwrap();
    let phase = definition
        .phases
        .iter()
        .find(|phase| phase.id == phase_id)
        .unwrap();
    let fields = phase
        .required_fields
        .iter()
        .map(|field| {
            let receipt = |status: &str, exit_code: i64, scopes: &[&str], target: &str| {
                serde_json::json!({
                    "command":"cargo test focused",
                    "target":target,
                    "status":status,
                    "exit_code":exit_code,
                    "fresh":true,
                    "skipped":false,
                    "scopes":scopes
                })
                .to_string()
            };
            (
                field.clone(),
                match field.as_str() {
                    "fit" => "bounded_understood".into(),
                    "parent" => "current_confirmed".into(),
                    "preflight" => "current_clear".into(),
                    "authority" | "authority_boundary" => "authorized".into(),
                    "route" => "none".into(),
                    "isolation" | "ownership" => "confirmed".into(),
                    "overlap" => "clear".into(),
                    "review_mode" => "self".into(),
                    "anti_pattern_review" => "reviewed_clear".into(),
                    "missing_proof" => "none".into(),
                    "target_binding" => "focused-target".into(),
                    "red_receipt" => {
                        receipt("failed_as_expected", 1, &["focused"], "focused-target")
                    }
                    "green_receipt" => receipt("passed", 0, &["focused"], "focused-target"),
                    "focused_proof" | "affected_proof" => {
                        receipt("passed", 0, &["focused", "affected"], "final-target")
                    }
                    "truth_level" => "local_verified".into(),
                    "promotion" => "no_promotion".into(),
                    "handoff" => "none".into(),
                    _ => "recorded".into(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let request = CompletePipelinePhase {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: phase.id.clone(),
        outcome: PipelinePhaseOutcome::Completed,
        transition: if phase_id == "K5" {
            PipelineTransition::Complete
        } else {
            PipelineTransition::Continue
        },
        output: PipelinePhaseOutputDraft {
            body: "bounded v0.7 phase evidence".into(),
            producer_context_id: "current-context".into(),
            fields,
            verdict: Some("pass".into()),
            dispositions: vec!["satisfied".into()],
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
        revisit_phase_id: None,
        escalation_target: None,
        terminal_result: (phase_id == "K5").then(|| PipelineTerminalResultDraft {
            summary: "bounded implementation locally verified".into(),
            evidence: vec![SliceResultEvidence {
                kind: "test".into(),
                reference: "final-target".into(),
                observation: "focused and affected proof passed".into(),
            }],
            scope_impact: "bounded target only".into(),
            remaining_work: "none".into(),
        }),
        publish_blocked_result: false,
        consumed_knowledge: None,
        research_checkpoint: None,
    };
    (definition, request)
}

#[test]
fn lightweight_v07_accepts_rework_route_and_rejects_disposition_mismatch() {
    use tect_domain::{PipelinePhaseOutcome, PipelineTransition};

    let (definition, mut request) = v07_completion("K3");
    request.output.verdict = Some("rework".into());
    request.output.dispositions = vec!["rework".into()];
    request.outcome = PipelinePhaseOutcome::WaitingInput;
    request.transition = PipelineTransition::Continue;
    request.revisit_phase_id = Some("K2".into());
    assert!(request.validate(&definition).is_ok());

    request.output.dispositions = vec!["satisfied".into()];
    assert_eq!(
        request.validate(&definition),
        Err(tect_domain::Error::InvalidArguments)
    );

    let (_, mut pass_with_rework) = v07_completion("K3");
    pass_with_rework.output.dispositions = vec!["rework".into()];
    assert_eq!(
        pass_with_rework.validate(&definition),
        Err(tect_domain::Error::InvalidArguments)
    );
}

#[test]
fn lightweight_v07_binds_independent_and_self_review_context() {
    use tect_domain::PipelineReviewerAttestation;

    let (definition, mut request) = v07_completion("K3");
    request
        .output
        .fields
        .insert("review_mode".into(), "independent".into());
    assert!(matches!(
        request.validate(&definition),
        Err(tect_domain::Error::Refused(refusal))
            if refusal.code == tect_domain::RefusalCode::InvalidOutput
                && refusal.path.as_deref() == Some("arguments.params.output.fields.review_mode")
    ));

    request.output.reviewer_context = Some(PipelineReviewerAttestation {
        reviewer_identity: "independent-reviewer".into(),
        reviewer_context_id: "current-context".into(),
        producer_context_ids: vec!["producer-context".into()],
        fresh_input: true,
    });
    assert!(request.validate(&definition).is_ok());

    request.output.reviewer_context = Some(PipelineReviewerAttestation {
        reviewer_identity: "fake-reviewer".into(),
        reviewer_context_id: "current-context".into(),
        producer_context_ids: vec!["current-context".into()],
        fresh_input: true,
    });
    assert_eq!(
        request.validate(&definition),
        Err(tect_domain::Error::InvalidArguments)
    );

    request
        .output
        .fields
        .insert("review_mode".into(), "self".into());
    assert_eq!(
        request.validate(&definition),
        Err(tect_domain::Error::InvalidArguments)
    );
    request.output.reviewer_context = None;
    assert!(request.validate(&definition).is_ok());
}

#[test]
fn legacy_required_disposition_behavior_is_unchanged() {
    use tect_domain::{PipelinePhaseOutcome, PipelineTransition};

    let (mut definition, mut request) = v07_completion("K3");
    definition.version = "0.6.0-compatibility-fixture".into();
    request.output.verdict = Some("rework".into());
    request.output.dispositions = vec!["rework".into()];
    request.outcome = PipelinePhaseOutcome::WaitingInput;
    request.transition = PipelineTransition::Continue;
    request.revisit_phase_id = Some("K2".into());
    assert_eq!(
        request.validate(&definition),
        Err(tect_domain::Error::InvalidArguments)
    );
}

#[test]
fn lightweight_v07_body_is_optional_but_legacy_body_remains_required() {
    let (definition, request) = v07_completion("K1");
    let mut omitted = serde_json::to_value(&request).unwrap();
    omitted["output"].as_object_mut().unwrap().remove("body");
    let omitted: tect_domain::CompletePipelinePhase = serde_json::from_value(omitted).unwrap();
    assert!(omitted.output.body.is_empty());
    assert!(omitted.validate(&definition).is_ok());
    assert!(
        serde_json::to_value(&omitted).unwrap()["output"]
            .get("body")
            .is_none()
    );

    let (_, explicit) = v07_completion("K1");
    assert_eq!(explicit.output.body, "bounded v0.7 phase evidence");
    assert!(explicit.validate(&definition).is_ok());
    assert_eq!(
        serde_json::to_value(&explicit).unwrap()["output"]["body"],
        "bounded v0.7 phase evidence"
    );

    let mut legacy = definition;
    legacy.version = "0.6.0-compatibility-fixture".into();
    assert_eq!(
        omitted.validate(&legacy),
        Err(tect_domain::Error::InvalidArguments)
    );
}

#[test]
fn lightweight_v07_compact_phase_requests_are_below_two_kibibytes() {
    use tect_domain::CompletePipelinePhase;

    let definition = lightweight_v07().unwrap();
    let mut sizes = Vec::new();
    for (index, phase) in definition.phases.iter().enumerate() {
        let (_, valid) = v07_completion(&phase.id);
        let fields = valid
            .output
            .fields
            .into_iter()
            .map(|(field, value)| (field, serde_json::json!(value)))
            .collect::<serde_json::Map<_, _>>();
        let mut params = serde_json::json!({
            "request_id":"00000000-0000-4000-8000-000000000001",
            "run_id":"00000000-0000-4000-8000-000000000002",
            "run_revision":index + 1,
            "phase_id":phase.id,
            "outcome":"completed",
            "transition":if phase.id == "K5" { "complete" } else { "continue" },
            "output":{
                "producer_context_id":"ctx",
                "fields":fields,
                "verdict":"pass",
                "dispositions":["satisfied"]
            }
        });
        if phase.id == "K5" {
            params["terminal_result"] = serde_json::json!({
                "summary":"x",
                "evidence":[{"kind":"test","reference":"x","observation":"x"}],
                "scope_impact":"x",
                "remaining_work":"none"
            });
        }
        let request: CompletePipelinePhase = serde_json::from_value(params.clone()).unwrap();
        assert!(request.validate(&definition).is_ok(), "{}", phase.id);
        let routed = serde_json::json!({
            "route":"slice.pipeline.phase.complete",
            "params":params
        });
        assert!(crate::api::decode_public_call("command", routed.clone()).is_ok());
        let params_bytes = serde_json::to_vec(&routed["params"]).unwrap().len();
        let routed_bytes = serde_json::to_vec(&routed).unwrap().len();
        assert!(routed_bytes <= 2 * 1024, "{}: {routed_bytes}", phase.id);
        sizes.push((phase.id.clone(), params_bytes, routed_bytes));
    }
    eprintln!("v07_phase_complete_request_bytes={sizes:?}");
}

#[test]
fn lightweight_v07_rejects_tampered_digest() {
    let source = include_str!("../../pipeline-definitions/lightweight-tdd-0.7.1-native.k1k5.json");
    let mut value: serde_json::Value = serde_json::from_str(source).unwrap();
    value["phases"][3]["required_fields"] = serde_json::json!(["red"]);
    let tampered = serde_json::to_string(&value).unwrap();
    assert!(load(&tampered, PipelineKind::LightweightTddDevelopment).is_err());
}

#[test]
fn lightweight_v07_has_resolvable_k_anchors_reference_adapters_and_exact_fields() {
    let definition = lightweight_v07().unwrap();
    let ledger = include_str!("../../../../skills/pipelines/lightweight-tdd/SOURCE-LEDGER.md");
    for phase in &definition.phases {
        assert!(phase.required_fields.len() <= 8, "{}", phase.id);
        let anchor = format!("## {}", phase.id);
        assert!(ledger.contains(&anchor), "missing {anchor}");
        assert!(
            phase.instructions[0]
                .origin_refs
                .iter()
                .any(|reference| reference.ends_with(&format!("#{}", phase.id)))
        );
    }
    for reference in [
        "using-git-worktrees",
        "test-driven-development",
        "testing-anti-patterns",
        "verification-before-completion",
        "writing-plans",
        "executing-plans",
        "systematic-debugging",
        "writing-skills",
        "finishing-a-development-branch",
    ] {
        assert!(
            ledger.contains(reference),
            "missing reference map for {reference}"
        );
    }
    assert_eq!(
        definition
            .phases
            .iter()
            .map(|phase| phase.required_fields.len())
            .collect::<Vec<_>>(),
        vec![7, 8, 7, 8, 8]
    );
}

#[test]
fn lightweight_v07_blocks_invalid_entry_preflight_and_tdd_receipts() {
    for (field, invalid) in [
        ("parent", "missing"),
        ("preflight", "conflicting"),
        ("authority", "unknown"),
    ] {
        let (definition, mut request) = v07_completion("K1");
        request.output.fields.insert(field.into(), invalid.into());
        assert!(matches!(
            request.validate(&definition),
            Err(tect_domain::Error::Refused(refusal))
                if refusal.code == tect_domain::RefusalCode::InvalidOutput
                    && refusal.path.as_deref() == Some(&format!("arguments.params.output.fields.{field}"))
        ));
    }
    for (field, invalid) in [
        ("isolation", "unknown"),
        ("ownership", "other_owned"),
        ("overlap", "conflicting"),
        ("route", "defer"),
        ("route", "unknown"),
    ] {
        let (definition, mut request) = v07_completion("K2");
        request.output.fields.insert(field.into(), invalid.into());
        assert!(matches!(
            request.validate(&definition),
            Err(tect_domain::Error::Refused(refusal))
                if refusal.code == tect_domain::RefusalCode::InvalidOutput
                    && refusal.path.as_deref() == Some(&format!("arguments.params.output.fields.{field}"))
        ));
    }
    let (definition, mut request) = v07_completion("K4");
    request.output.fields.insert(
        "red_receipt".into(),
        serde_json::json!({"command":"cargo test focused","target":"focused-target","status":"failed_as_expected","exit_code":0,"fresh":true,"skipped":false,"scopes":["focused"]}).to_string(),
    );
    assert!(matches!(
        request.validate(&definition),
        Err(tect_domain::Error::Refused(refusal))
            if refusal.path.as_deref() == Some("arguments.params.output.fields.red_receipt")
    ));
    let (_, mut request) = v07_completion("K4");
    request.output.fields.insert(
        "green_receipt".into(),
        serde_json::json!({"command":"cargo test focused","target":"other-target","status":"passed","exit_code":0,"fresh":true,"skipped":false,"scopes":["focused"]}).to_string(),
    );
    assert!(matches!(
        request.validate(&definition),
        Err(tect_domain::Error::Refused(refusal))
            if refusal.path.as_deref() == Some("arguments.params.output.fields.green_receipt")
    ));
}

#[test]
fn lightweight_v07_blocks_invalid_local_proof_and_open_handoff() {
    for (field, mutation) in [
        ("focused_proof", (false, false, 0, vec!["focused"])),
        ("affected_proof", (true, false, 1, vec!["affected"])),
        ("affected_proof", (true, true, 0, vec!["affected"])),
        ("affected_proof", (true, false, 0, vec!["focused"])),
    ] {
        let (definition, mut request) = v07_completion("K5");
        let (fresh, skipped, exit_code, scopes) = mutation;
        request.output.fields.insert(
            field.into(),
            serde_json::json!({"command":"cargo test final","target":"final-target","status":"passed","exit_code":exit_code,"fresh":fresh,"skipped":skipped,"scopes":scopes}).to_string(),
        );
        assert!(matches!(
            request.validate(&definition),
            Err(tect_domain::Error::Refused(refusal))
                if refusal.code == tect_domain::RefusalCode::InvalidOutput
                    && refusal.path.as_deref() == Some(&format!("arguments.params.output.fields.{field}"))
        ));
    }
    for (field, value) in [("missing_proof", "deferred"), ("handoff", "active")] {
        let (definition, mut request) = v07_completion("K5");
        request.output.fields.insert(field.into(), value.into());
        assert!(matches!(
            request.validate(&definition),
            Err(tect_domain::Error::Refused(refusal))
                if refusal.path.as_deref() == Some(&format!("arguments.params.output.fields.{field}"))
        ));
    }
}

#[test]
fn lightweight_v07_accepts_one_bounded_k1_through_k5_contract_path() {
    for phase_id in ["K1", "K2", "K3", "K4", "K5"] {
        let (definition, request) = v07_completion(phase_id);
        assert!(request.validate(&definition).is_ok(), "{phase_id}");
    }
}

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
fn explicit_definition_selection_keeps_v06_default_and_exposes_v07_k1k5() {
    let provider = StaticPipelineDefinitions;
    let legacy = provider
        .definition(PipelineKind::LightweightTddDevelopment)
        .unwrap();
    let explicit_legacy = provider
        .definition_for(
            PipelineKind::LightweightTddDevelopment,
            Some("0.6.0-native.engineering.2"),
        )
        .unwrap();
    assert_eq!(explicit_legacy.version, legacy.version);
    assert_eq!(explicit_legacy.digest, legacy.digest);

    let compact = provider
        .definition_for(
            PipelineKind::LightweightTddDevelopment,
            Some("0.7.1-native.k1k5"),
        )
        .unwrap();
    assert_eq!(compact.phases.len(), 5);
    assert_eq!(compact.version, "0.7.1-native.k1k5");
    assert!(
        compact.phases[2].instructions[0]
            .body
            .contains("apply a mandatory counterfactual")
    );
    assert!(
        compact.phases[2].instructions[0]
            .body
            .contains("Result must not complete")
    );

    let previous = provider
        .definition_for(
            PipelineKind::LightweightTddDevelopment,
            Some("0.7.0-native.k1k5"),
        )
        .unwrap();
    assert_eq!(previous.version, "0.7.0-native.k1k5");
    assert_eq!(
        previous.digest,
        "7f5dd6a4503078538d45d0c90c83fdcd896ff1216167556ff9bd0424f826aab0"
    );

    assert!(
        provider
            .definition_for(PipelineKind::LightweightTddDevelopment, Some("0.7.0"))
            .is_err()
    );
    assert!(
        provider
            .definition_for(
                PipelineKind::FullDesignToExecution,
                Some("0.7.0-native.k1k5")
            )
            .is_err()
    );
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
        include_str!("../../pipeline-definitions/lightweight-tdd-0.1.0-native.1.json"),
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
