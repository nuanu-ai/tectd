use super::*;

#[test]
fn requirement_ledger_reports_duplicate_identity_inventory_and_modality_violations_together() {
    let body = serde_json::json!({
        "source":{"path":"spec.md","digest":"abc"},
        "sourceRequirementIds":["REQ-001","REQ-001","REQ-002"],
        "requirements":[
            {"id":"REQ-001","modality":"MUST"},
            {"id":"REQ-001","modality":"SHOULD"},
            {"id":"REQ-003","modality":"INVALID"}
        ]
    })
    .to_string();
    let error =
        parse_requirements_ledger(&body, "slice-component-decision-interrogator").unwrap_err();
    let diagnostic = error.pipeline_artifact_diagnostic().unwrap();
    assert_eq!(diagnostic.code, "requirement_ledger_invalid");
    assert_eq!(diagnostic.phase, "slice-component-decision-interrogator");
    assert_eq!(diagnostic.artifact, "requirements-ledger.json");
    assert!(diagnostic.retryable);
    assert_eq!(
        diagnostic
            .violations
            .iter()
            .map(|violation| violation.code.as_str())
            .collect::<Vec<_>>(),
        vec![
            "source_requirement_id_duplicate",
            "requirement_id_duplicate",
            "requirement_modality_invalid",
            "source_requirement_missing_row",
        ]
    );
    assert_eq!(diagnostic.violations[2].path, "$.requirements[2].modality");
    assert_eq!(
        diagnostic.violations[2].actual.as_deref(),
        Some("string:INVALID")
    );
    assert!(!diagnostic.truncated);
    assert_eq!(diagnostic.omitted_violation_count, 0);
}

#[test]
fn malformed_and_large_raw_values_produce_bounded_stable_diagnostics() {
    let malformed = format!("{{\"payload\":\"{}\"", "x".repeat(500_000));
    let error = parse_requirements_ledger(&malformed, "phase-seven").unwrap_err();
    let diagnostic = error.pipeline_artifact_diagnostic().unwrap();
    assert_eq!(diagnostic.code, "requirement_ledger_invalid");
    assert_eq!(diagnostic.violations[0].code, "invalid_json");
    assert_eq!(
        diagnostic.violations[0].actual.as_deref(),
        Some("malformed_json")
    );
    assert!(serde_json::to_vec(diagnostic).unwrap().len() < 2_048);

    let raw = serde_json::json!({
        "source":{"path":"spec.md","digest":"abc"},
        "sourceRequirementIds":[{"raw":"x".repeat(500_000)}],
        "requirements":[]
    })
    .to_string();
    let error = parse_requirements_ledger(&raw, "phase-seven").unwrap_err();
    let diagnostic = error.pipeline_artifact_diagnostic().unwrap();
    assert_eq!(
        diagnostic.violations[0].code,
        "source_requirement_id_invalid"
    );
    assert_eq!(
        diagnostic.violations[0].actual.as_deref(),
        Some("object(keys=1)")
    );
    assert!(!diagnostic.truncated);
    assert_eq!(diagnostic.omitted_violation_count, 0);
    assert!(serde_json::to_vec(diagnostic).unwrap().len() < 2_048);
}

#[test]
fn high_cardinality_diffs_are_capped_with_an_exact_omitted_count() {
    let ids = (0..10_000)
        .map(|index| format!("REQ-{index:03}"))
        .collect::<Vec<_>>();
    let body = serde_json::json!({
        "source":{"path":"spec.md","digest":"abc"},
        "sourceRequirementIds":ids,
        "requirements":[]
    })
    .to_string();
    let first = parse_requirements_ledger(&body, "phase-five").unwrap_err();
    let second = parse_requirements_ledger(&body, "phase-five").unwrap_err();
    let first = first.pipeline_artifact_diagnostic().unwrap();
    let second = second.pipeline_artifact_diagnostic().unwrap();
    assert_eq!(first.code, "requirement_ledger_invalid");
    assert_eq!(
        first.violations.len(),
        MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VIOLATIONS
    );
    assert_eq!(first.omitted_violation_count, 9_976);
    assert!(first.truncated);
    assert_eq!(first, second);
    assert!(serde_json::to_vec(first).unwrap().len() < 16_384);
}

#[test]
fn legacy_wrapper_preserves_nested_truncation_and_omitted_count() {
    let violations = (0..40)
        .map(|index| {
            violation(
                "source_requirement_missing_row",
                format!("$.requirements[{index}]"),
                Some(format!("row for REQ-{index:03}")),
                None,
            )
        })
        .collect::<Vec<_>>();
    let nested = Error::InvalidPipelineArtifact(Box::new(PipelineArtifactDiagnostic::bounded(
        "requirement_ledger_invalid".to_owned(),
        "slice-component-decision-interrogator".to_owned(),
        "requirements-ledger.json".to_owned(),
        violations,
        true,
        "Correct phase 5.".to_owned(),
    )));
    let wrapped = wrap_prior_ledger_error(nested);
    let diagnostic = wrapped.pipeline_artifact_diagnostic().unwrap();
    assert_eq!(diagnostic.code, "requirement_ledger_lineage_invalid");
    assert_eq!(diagnostic.violations.len(), 24);
    assert_eq!(diagnostic.omitted_violation_count, 16);
    assert!(diagnostic.truncated);
    assert!(
        diagnostic
            .violations
            .iter()
            .all(|violation| violation.path.starts_with("phase5:"))
    );
    assert!(serde_json::to_vec(diagnostic).unwrap().len() < 16_384);
}
