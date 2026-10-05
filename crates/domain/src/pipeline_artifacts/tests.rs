use super::*;
use serde_json::{Value, json};

fn phase() -> PipelinePhaseDefinition {
    serde_json::from_value(phase_value()).unwrap()
}
fn phase_value() -> Value {
    json!({"id":"review", "ordinal":1, "title":"review", "required":true,
    "disposition_required":false, "instructions":[], "skills":[], "resources":[{"id":"validator", "version":"1", "digest":"vd", "body":"validator", "origin_refs":[]}],
    "required_artifacts":[{"name_pattern":"review.md", "media_type":"text/markdown", "required":true, "minimum_matches":1}],
    "validator_contracts":[{"resource_id":"validator", "version":"1", "digest":"vd", "stage":"review", "artifact_patterns":["review.md"], "required_verdicts":["PASS"], "success_verdicts":["PASS"]}],
    "required_fields":[], "allowed_verdicts":["PASS","REWORK"], "required_dispositions":[], "allowed_backward_to":[], "fresh_reviewer_input":false, "retry_policy":"repeatable", "output_contract":"review"})
}
fn artifact() -> Value {
    json!({"name":"review.md", "media_type":"text/markdown", "body":"report", "digest":"ad", "reference":null})
}
fn output_value() -> Value {
    json!({"producer_context_id":"ctx", "verdict":"PASS", "artifacts":[artifact()], "validator_receipts":[{
    "resource_id":"validator", "version":"1", "digest":"vd", "stage":"review", "command":"validate", "exit_code":0, "valid":true,
    "artifacts":[{"name":"review.md","digest":"ad"}]}]})
}
fn output(value: Value) -> PipelinePhaseOutputDraft {
    serde_json::from_value(value).unwrap()
}
fn assert_rule(error: Error, rule: &str, path: &str, code: RefusalCode) {
    let refusal = error.refusal().unwrap();
    assert_eq!(refusal.code, code);
    assert_eq!(refusal.rule.as_deref(), Some(rule));
    assert_eq!(refusal.path.as_deref(), Some(path));
    assert!(refusal.expected.is_some());
    assert!(refusal.actual.is_some());
    assert!(refusal.required.is_some());
}

#[test]
fn artifact_names_are_safe_and_patterns_are_bounded() {
    assert!(valid_pattern("decisions/*.md"));
    assert!(pattern_matches("decisions/*.md", "decisions/api.md"));
    assert!(!pattern_matches("decisions/*.md", "decisions/../api.md"));
    assert!(!valid_pattern("**/*.md"));
    assert!(!valid_name("../escape.json"));
}
#[test]
fn artifacts_and_validator_receipts_accept_valid_and_optional_not_run() {
    let phase = phase();
    assert!(validate_artifact_definition(&phase).is_ok());
    assert!(validate_artifacts(&phase, &output(output_value())).is_ok());
    let mut value = output_value();
    value["verdict"] = json!("REWORK");
    value["validator_receipts"] = json!([]);
    assert!(validate_artifacts(&phase, &output(value.clone())).is_ok());
    value["validator_receipts"] = output_value()["validator_receipts"].clone();
    value["validator_receipts"][0]["command"] = json!("not_run");
    value["validator_receipts"][0]["valid"] = json!(false);
    value["validator_receipts"][0]["artifacts"] = json!([]);
    assert!(validate_artifacts(&phase, &output(value)).is_ok());
}
#[test]
fn artifact_metadata_each_field_and_first_violation_have_indexed_safe_refusals() {
    for (field, changed, suffix) in [
        ("name", json!("../秘密.md"), "NAME"),
        ("media_type", json!(" "), "MEDIA"),
        ("body", json!(""), "BODY"),
        ("digest", json!(" "), "DIGEST"),
        ("reference", json!(" "), "REFERENCE"),
    ] {
        let mut value = output_value();
        value["artifacts"][0][field] = changed;
        let error = validate_artifacts(&phase(), &output(value)).unwrap_err();
        assert!(!serde_json::to_string(&error).unwrap().contains("秘密"));
        assert_rule(
            error,
            &format!("WP6-ARTIFACT-{suffix}"),
            &format!("output.artifacts[0].{field}"),
            RefusalCode::InvalidOutput,
        );
    }
    let mut value = output_value();
    value["artifacts"][0]["name"] = json!("../bad");
    value["artifacts"][0]["body"] = json!("");
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-ARTIFACT-NAME",
        "output.artifacts[0].name",
        RefusalCode::InvalidOutput,
    );
    let mut value = output_value();
    value["artifacts"] = json!([artifact(), artifact()]);
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-ARTIFACT-DUPLICATE",
        "output.artifacts[1].name",
        RefusalCode::InvalidOutput,
    );
}
#[test]
fn artifact_requirements_name_media_and_count_are_distinct() {
    for (field, changed, suffix) in [
        ("name", json!("unknown.md"), "REQUIREMENT"),
        ("media_type", json!("text/plain"), "REQUIREMENT-MEDIA"),
    ] {
        let mut value = output_value();
        value["artifacts"][0][field] = changed;
        assert_rule(
            validate_artifacts(&phase(), &output(value)).unwrap_err(),
            &format!("WP6-ARTIFACT-{suffix}"),
            &format!("output.artifacts[0].{field}"),
            RefusalCode::InvalidOutput,
        );
    }
    let mut value = output_value();
    value["artifacts"] = json!([]);
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-ARTIFACT-REQUIRED-COUNT",
        "output.artifacts",
        RefusalCode::InvalidOutput,
    );
}
#[test]
fn receipt_duplicates_unknown_and_missing_preserve_first_violation_order() {
    let mut value = output_value();
    value["validator_receipts"] = json!([]);
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-MISSING",
        "output.validator_receipts",
        RefusalCode::InvalidOutput,
    );
    let mut value = output_value();
    value["validator_receipts"][0]["resource_id"] = json!("unknown");
    assert_rule(
        validate_artifacts(&phase(), &output(value.clone())).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-UNKNOWN",
        "output.validator_receipts[0].resource_id",
        RefusalCode::InvalidOutput,
    );
    let receipt = value["validator_receipts"][0].clone();
    value["validator_receipts"] = json!([receipt.clone(), receipt]);
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-DUPLICATE",
        "output.validator_receipts[1].resource_id",
        RefusalCode::InvalidOutput,
    );
}
#[test]
fn receipt_contract_metadata_artifact_set_and_success_are_exact() {
    for (field, changed, suffix) in [
        ("version", json!("wrong"), "VERSION"),
        ("digest", json!("wrong"), "DIGEST"),
        ("command", json!(" "), "COMMAND"),
        ("artifacts", json!([]), "ARTIFACT-SET"),
        ("exit_code", json!(1), "SUCCESS-EXIT"),
        ("valid", json!(false), "SUCCESS-VALID"),
    ] {
        let mut value = output_value();
        value["validator_receipts"][0][field] = changed;
        assert_rule(
            validate_artifacts(&phase(), &output(value)).unwrap_err(),
            &format!("WP6-VALIDATOR-RECEIPT-{suffix}"),
            &format!("output.validator_receipts[0].{field}"),
            RefusalCode::InvalidOutput,
        );
    }
    let mut value = output_value();
    value["validator_receipts"][0]["version"] = json!("wrong");
    value["validator_receipts"][0]["digest"] = json!("wrong");
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-VERSION",
        "output.validator_receipts[0].version",
        RefusalCode::InvalidOutput,
    );
    let mut value = output_value();
    let item = value["validator_receipts"][0]["artifacts"][0].clone();
    value["validator_receipts"][0]["artifacts"] = json!([item.clone(), item]);
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-ARTIFACT-DUPLICATE",
        "output.validator_receipts[0].artifacts[1]",
        RefusalCode::InvalidOutput,
    );
    let mut value = output_value();
    value["validator_receipts"][0]["artifacts"][0]["digest"] = json!("other");
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-ARTIFACT-SET",
        "output.validator_receipts[0].artifacts",
        RefusalCode::InvalidOutput,
    );
}
#[test]
fn receipt_not_run_required_valid_and_artifacts_are_separate() {
    let mut value = output_value();
    value["validator_receipts"][0]["command"] = json!("not_run");
    assert_rule(
        validate_artifacts(&phase(), &output(value.clone())).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-NOT-RUN-REQUIRED",
        "output.validator_receipts[0].command",
        RefusalCode::InvalidOutput,
    );
    value["verdict"] = json!("REWORK");
    assert_rule(
        validate_artifacts(&phase(), &output(value.clone())).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-NOT-RUN-VALID",
        "output.validator_receipts[0].valid",
        RefusalCode::InvalidOutput,
    );
    value["validator_receipts"][0]["valid"] = json!(false);
    assert_rule(
        validate_artifacts(&phase(), &output(value)).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-NOT-RUN-ARTIFACTS",
        "output.validator_receipts[0].artifacts",
        RefusalCode::InvalidOutput,
    );
}
#[test]
fn artifact_definition_branches_use_shape_refusals() {
    for (field, changed, suffix) in [
        ("name_pattern", json!("**/*.md"), "NAME"),
        ("media_type", json!(" "), "MEDIA"),
        ("media_type", json!("application/json"), "SCHEMA"),
        ("minimum_matches", json!(0), "COUNT"),
        ("when_verdict", json!("unknown"), "VERDICT"),
    ] {
        let mut value = phase_value();
        value["required_artifacts"][0][field] = changed;
        let error =
            validate_artifact_definition(&serde_json::from_value(value).unwrap()).unwrap_err();
        let path_field = if suffix == "SCHEMA" {
            "schema_ref"
        } else {
            field
        };
        assert_rule(
            error,
            &format!("WP6-ARTIFACT-DEFINITION-{suffix}"),
            &format!("required_artifacts[0].{path_field}"),
            RefusalCode::InputSchemaInvalid,
        );
    }
    let mut value = phase_value();
    let req = value["required_artifacts"][0].clone();
    value["required_artifacts"] = json!([req.clone(), req]);
    assert_rule(
        validate_artifact_definition(&serde_json::from_value(value).unwrap()).unwrap_err(),
        "WP6-ARTIFACT-DEFINITION-DUPLICATE",
        "required_artifacts[1].name_pattern",
        RefusalCode::InputSchemaInvalid,
    );
}
#[test]
fn validator_definition_fields_lists_and_contract_consistency_are_distinct() {
    for (field, changed, suffix, path_field) in [
        ("resource_id", json!(" "), "RESOURCE", "resource_id"),
        ("version", json!(" "), "VERSION", "version"),
        ("digest", json!(" "), "DIGEST", "digest"),
        ("stage", json!(" "), "STAGE", "stage"),
        (
            "artifact_patterns",
            json!([]),
            "PATTERNS",
            "artifact_patterns",
        ),
        ("success_verdicts", json!([]), "SUCCESS", "success_verdicts"),
        (
            "artifact_patterns",
            json!(["review.md", "review.md"]),
            "PATTERN-DUPLICATE",
            "artifact_patterns[1]",
        ),
        (
            "required_verdicts",
            json!(["PASS", "PASS"]),
            "REQUIRED-DUPLICATE",
            "required_verdicts[1]",
        ),
        (
            "success_verdicts",
            json!(["PASS", "PASS"]),
            "SUCCESS-DUPLICATE",
            "success_verdicts[1]",
        ),
        (
            "artifact_patterns",
            json!(["../bad"]),
            "PATTERN",
            "artifact_patterns[0]",
        ),
        (
            "artifact_patterns",
            json!(["other.md"]),
            "PATTERN-REQUIREMENT",
            "artifact_patterns[0]",
        ),
        (
            "success_verdicts",
            json!(["unknown"]),
            "SUCCESS-VERDICT",
            "success_verdicts[0]",
        ),
        (
            "required_verdicts",
            json!(["unknown"]),
            "REQUIRED-VERDICT",
            "required_verdicts[0]",
        ),
        (
            "required_verdicts",
            json!(["REWORK"]),
            "SUCCESS-REQUIRED",
            "success_verdicts[0]",
        ),
        ("version", json!("2"), "RESOURCE-MATCH", "resource_id"),
    ] {
        let mut value = phase_value();
        value["validator_contracts"][0][field] = changed;
        assert_rule(
            validate_artifact_definition(&serde_json::from_value(value).unwrap()).unwrap_err(),
            &format!("WP6-VALIDATOR-RECEIPT-DEFINITION-{suffix}"),
            &format!("validator_contracts[0].{path_field}"),
            RefusalCode::InputSchemaInvalid,
        );
    }
    let mut value = phase_value();
    let contract = value["validator_contracts"][0].clone();
    value["validator_contracts"] = json!([contract.clone(), contract]);
    assert_rule(
        validate_artifact_definition(&serde_json::from_value(value).unwrap()).unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-DEFINITION-DUPLICATE",
        "validator_contracts[1].resource_id",
        RefusalCode::InputSchemaInvalid,
    );
}

#[test]
fn definition_validation_maps_artifact_and_validator_paths_to_actual_later_phase() {
    let instruction = PipelineInstructionSnapshot {
        id: "instruction".into(),
        version: "1".into(),
        digest: "digest".into(),
        body: "body".into(),
        origin_refs: vec!["source".into()],
    };
    let mut later = phase();
    later.id = "later".into();
    later.ordinal = 2;
    later.instructions = vec![instruction.clone()];
    later.resources.clear();
    later.allowed_verdicts.clear();
    later.validator_contracts.clear();
    let mut earlier = later.clone();
    earlier.id = "earlier".into();
    earlier.ordinal = 1;
    earlier.required_artifacts.clear();
    let mut definition = PipelineDefinitionSnapshot {
        kind: PipelineKind::DebugRootCause,
        version: "1".into(),
        digest: "digest".into(),
        overview: instruction,
        default_mode: PipelineDeliveryMode::Phasewise,
        allowed_modes: vec![PipelineDeliveryMode::Phasewise],
        phases: vec![earlier, later],
        completion_contract: "complete".into(),
        escalation_contract: "escalate".into(),
        forbidden_claims: vec![],
    };
    definition.phases[1].required_artifacts[0].name_pattern = "../unsafe".into();
    assert_rule(
        definition.validate().unwrap_err(),
        "WP6-ARTIFACT-DEFINITION-NAME",
        "pipeline_definition.phases[1].required_artifacts[0].name_pattern",
        RefusalCode::InputSchemaInvalid,
    );
    definition.phases[1].required_artifacts[0].name_pattern = "review.md".into();
    definition.phases[1].validator_contracts = phase().validator_contracts;
    definition.phases[1].validator_contracts[0].resource_id = " ".into();
    assert_rule(
        definition.validate().unwrap_err(),
        "WP6-VALIDATOR-RECEIPT-DEFINITION-RESOURCE",
        "pipeline_definition.phases[1].validator_contracts[0].resource_id",
        RefusalCode::InputSchemaInvalid,
    );
}

#[test]
fn definition_pointer_mapper_preserves_unrelated_errors_and_all_wrapped_metadata() {
    let refusal = Refusal::new(RefusalCode::InputSchemaInvalid)
        .with_rule("WP6-ARTIFACT-DEFINITION-NAME")
        .with_path("required_artifacts[2].name_pattern")
        .with_expected("safe pattern")
        .with_actual("unsafe pattern")
        .with_message("message")
        .with_next_action("correct_input_and_retry")
        .with_required("schema_valid_input");
    let mut expected_refusal = refusal.clone();
    expected_refusal.path =
        Some("pipeline_definition.phases[3].required_artifacts[2].name_pattern".into());
    assert_eq!(
        prefix_definition_refusal(Error::Refused(Box::new(refusal.clone())), 3),
        Error::Refused(Box::new(expected_refusal.clone()))
    );
    let mut unrelated = refusal.clone();
    unrelated.path = Some("output.artifacts[0].name".into());
    let error = Error::Refused(Box::new(unrelated.clone()));
    assert_eq!(prefix_definition_refusal(error.clone(), 3), error);
    let already_mapped = Error::Refused(Box::new(expected_refusal.clone()));
    assert_eq!(
        prefix_definition_refusal(already_mapped.clone(), 3),
        already_mapped
    );
    assert_eq!(
        prefix_definition_refusal(Error::StorageUnavailable, 3),
        Error::StorageUnavailable
    );
    // Both nested refusal metadata and the outer wrapper survive; only local definition paths change.
    let wrapped = Error::PipelineRefused {
        source: Box::new(Error::PipelineRefused {
            source: Box::new(Error::InvalidArguments),
            refusal: Box::new(refusal),
        }),
        refusal: Box::new(unrelated.clone()),
    };
    let expected = Error::PipelineRefused {
        source: Box::new(Error::PipelineRefused {
            source: Box::new(Error::InvalidArguments),
            refusal: Box::new(expected_refusal),
        }),
        refusal: Box::new(unrelated),
    };
    assert_eq!(prefix_definition_refusal(wrapped, 3), expected);
}
