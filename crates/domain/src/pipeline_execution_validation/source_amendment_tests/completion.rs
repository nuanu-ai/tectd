use super::*;

#[test]
fn backend_proof_is_rejected_only_for_current_definitions_with_exact_field_paths() {
    let legacy = proof_test_completion();
    assert_eq!(legacy.validate(&proof_test_definition("0.6.0")), Ok(()));

    let definition = proof_test_definition("0.7.0-native.k1k5");
    for (field, path) in [
        ("consumed_outputs", "arguments.params.consumed_outputs"),
        ("consumed_inputs", "arguments.params.consumed_inputs"),
    ] {
        let mut request = proof_test_completion();
        if field == "consumed_outputs" {
            request.consumed_inputs.clear();
        } else {
            request.consumed_outputs.clear();
        }
        let error = request.validate(&definition).unwrap_err();
        let refusal = error.refusal().expect("typed backend proof refusal");
        assert_eq!(error.code(), "BACKEND_DERIVED_PROOF_REQUIRED");
        assert_eq!(refusal.code, RefusalCode::BackendDerivedProofRequired);
        assert_eq!(refusal.rule.as_deref(), Some("WP3-PROOF-01"));
        assert_eq!(refusal.path.as_deref(), Some(path));
        assert_eq!(
            refusal.next_action.as_deref(),
            Some("omit_agent_supplied_proof")
        );
        assert_eq!(
            refusal.expected.as_deref(),
            Some("omitted; backend derives the proof")
        );
    }
}

#[test]
fn legacy_resource_read_digest_mismatch_is_a_precise_invalid_output_refusal() {
    let mut definition = proof_test_definition("0.6.0");
    definition.phases[0]
        .resources
        .push(PipelineInstructionSnapshot {
            id: "test-resource".into(),
            version: "1".into(),
            digest: "expected-digest".into(),
            body: "resource".into(),
            origin_refs: Vec::new(),
        });
    let mut request = proof_test_completion();
    request
        .output
        .resource_reads
        .push(PipelineSkillReadReceipt {
            instruction_id: "test-resource".into(),
            version: "1".into(),
            digest: "expected-digest".into(),
        });
    assert_eq!(request.validate(&definition), Ok(()));

    request.output.resource_reads[0].digest = "substituted-digest".into();
    let error = request.validate(&definition).unwrap_err();
    let refusal = error.refusal().expect("typed resource-read refusal");
    assert_eq!(error.code(), "INVALID_OUTPUT");
    assert_eq!(refusal.code, RefusalCode::InvalidOutput);
    assert_eq!(refusal.rule.as_deref(), Some("WP6-RESOURCE-READ-01"));
    assert_eq!(
        refusal.path.as_deref(),
        Some("arguments.params.output.resource_reads")
    );
    let expected: serde_json::Value =
        serde_json::from_str(refusal.expected.as_ref().unwrap()).unwrap();
    assert_eq!(expected["missing"][0]["instruction_id"], "test-resource");
    assert_eq!(expected["missing"][0]["digest"], "expected-digest");
    let actual: serde_json::Value = serde_json::from_str(refusal.actual.as_ref().unwrap()).unwrap();
    assert_eq!(actual["unexpected"][0]["digest"], "substituted-digest");
    assert!(actual["duplicates"].as_array().unwrap().is_empty());
    assert_eq!(
        refusal.next_action.as_deref(),
        Some("supply_exact_phase_resource_reads")
    );
    assert_eq!(
        refusal.required.as_deref(),
        Some("exact_phase_resource_reads")
    );
}

#[test]
fn completion_refusals_identify_request_output_and_reviewer_fields_in_order() {
    let definition = proof_test_definition("0.6.0");
    let base = serde_json::to_value(proof_test_completion()).unwrap();
    for (path, value, rule) in [
        (
            "/request_id",
            serde_json::json!(Uuid::nil()),
            "WP6-COMPLETE-REQUEST-01",
        ),
        (
            "/run_id",
            serde_json::json!(Uuid::nil()),
            "WP6-COMPLETE-REQUEST-02",
        ),
        (
            "/run_revision",
            serde_json::json!(0),
            "WP6-COMPLETE-REQUEST-03",
        ),
        (
            "/phase_id",
            serde_json::json!("absent"),
            "WP6-COMPLETE-REQUEST-06",
        ),
        (
            "/output/body",
            serde_json::json!(" "),
            "WP6-COMPLETE-OUTPUT-01",
        ),
        (
            "/output/producer_context_id",
            serde_json::json!(" "),
            "WP6-COMPLETE-OUTPUT-02",
        ),
        (
            "/output/producer_context_id",
            serde_json::json!("x".repeat(MAX_PIPELINE_CONTEXT_ID_BYTES + 1)),
            "WP6-COMPLETE-OUTPUT-03",
        ),
        (
            "/output/reference",
            serde_json::json!(" "),
            "WP6-COMPLETE-OUTPUT-04",
        ),
        (
            "/consumed_outputs/0/output_revision",
            serde_json::json!(0),
            "WP6-COMPLETE-REQUEST-04-2",
        ),
        (
            "/consumed_inputs/0/digest",
            serde_json::json!(""),
            "WP6-COMPLETE-REQUEST-05-3",
        ),
    ] {
        let mut value_json = base.clone();
        *value_json.pointer_mut(path).unwrap() = value;
        let request: CompletePipelinePhase = serde_json::from_value(value_json).unwrap();
        let refusal = request
            .validate(&definition)
            .unwrap_err()
            .refusal()
            .unwrap();
        assert_eq!(refusal.rule.as_deref(), Some(rule));
        assert_eq!(
            refusal.path.unwrap(),
            format!(
                "arguments.params{}",
                path.replace('/', ".").replace(".0.", "[0].")
            )
        );
    }
    let mut request = proof_test_completion();
    request.request_id = Uuid::nil();
    request.run_id = Uuid::nil();
    assert_eq!(
        request
            .validate(&definition)
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-COMPLETE-REQUEST-01")
    );
    let context = serde_json::json!({"reviewer_identity":"reviewer", "reviewer_context_id":"ctx", "producer_context_ids":["producer"], "fresh_input":true});
    for (field, value, rule) in [
        (
            "reviewer_identity",
            serde_json::json!(""),
            "WP6-REVIEW-CONTEXT-01",
        ),
        (
            "reviewer_context_id",
            serde_json::json!("other"),
            "WP6-REVIEW-CONTEXT-05",
        ),
        (
            "producer_context_ids",
            serde_json::json!([]),
            "WP6-REVIEW-PRODUCERS-01",
        ),
        (
            "producer_context_ids",
            serde_json::json!([""]),
            "WP6-REVIEW-PRODUCERS-03",
        ),
        (
            "producer_context_ids",
            serde_json::json!(["ctx"]),
            "WP6-REVIEW-PRODUCERS-05",
        ),
        (
            "producer_context_ids",
            serde_json::json!(["producer", "producer"]),
            "WP6-REVIEW-PRODUCERS-06",
        ),
        (
            "fresh_input",
            serde_json::json!(false),
            "WP6-REVIEW-CONTEXT-06",
        ),
    ] {
        let mut c = context.clone();
        c[field] = value;
        let mut request = proof_test_completion();
        request.output.reviewer_context = Some(serde_json::from_value(c).unwrap());
        assert_eq!(
            request
                .validate(&definition)
                .unwrap_err()
                .refusal()
                .unwrap()
                .rule
                .as_deref(),
            Some(rule)
        );
    }
}
