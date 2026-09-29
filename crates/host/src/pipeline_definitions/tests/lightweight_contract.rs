use super::*;

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
    let source =
        include_str!("../../../pipeline-definitions/lightweight-tdd-0.7.1-native.k1k5.json");
    let mut value: serde_json::Value = serde_json::from_str(source).unwrap();
    value["phases"][3]["required_fields"] = serde_json::json!(["red"]);
    let tampered = serde_json::to_string(&value).unwrap();
    assert!(load(&tampered, PipelineKind::LightweightTddDevelopment).is_err());
}

#[test]
fn lightweight_v07_has_resolvable_k_anchors_reference_adapters_and_exact_fields() {
    let definition = lightweight_v07().unwrap();
    let ledger = include_str!("../../../../../skills/pipelines/lightweight-tdd/SOURCE-LEDGER.md");
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
