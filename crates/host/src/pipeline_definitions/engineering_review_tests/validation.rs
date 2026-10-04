use super::*;

fn report(request: &CompletePipelinePhase) -> Value {
    serde_json::from_str(&request.output.artifacts[0].body).unwrap()
}

fn replace_report(request: &mut CompletePipelinePhase, report: Value) {
    let body = serde_json::to_string(&report).unwrap();
    request.output.artifacts[0].digest = digest(&body);
    request.output.artifacts[0].body = body;
}

#[test]
fn engineering_review_accepts_complete_plan_and_size_boundaries() {
    let (definition, request) = completion();
    assert!(request.validate(&definition).is_ok());
    for (lines, kind) in [(501, "behavioral"), (1500, "declarative")] {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        value["files"][0]["line_count"] = json!(lines);
        value["files"][0]["content_kind"] = json!(kind);
        value["files"][0]["justification"] =
            json!("One cohesive responsibility requires this unit.");
        replace_report(&mut request, value);
        assert!(request.validate(&definition).is_ok(), "{lines}");
    }
}

#[test]
fn engineering_review_rejects_missing_receipt_report_or_exact_binding() {
    let (definition, mut request) = completion();
    request.output.resource_reads.pop();
    assert!(request.validate(&definition).is_err());
    let (definition, mut request) = completion();
    request.output.artifacts.clear();
    assert!(request.validate(&definition).is_err());
    let (definition, mut request) = completion();
    request.consumed_outputs[0].digest = "changed".into();
    assert!(request.validate(&definition).is_err());
}

#[test]
fn engineering_review_rejects_rule_verdict_finding_and_size_failures() {
    for mutation in 0..7 {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        match mutation {
            0 => value["rules_digest"] = json!("wrong"),
            1 => {
                value["assessments"].as_array_mut().unwrap().pop();
            }
            2 => {
                value["findings"] = json!([{"id":"F-1","rule_id":"ENG-02","status":"open","evidence":"Responsibility is split."}])
            }
            3 => value["verdict"] = json!("rework"),
            4 => value["files"][0]["line_count"] = json!(501),
            5 => {
                value["files"][0]["line_count"] = json!(1001);
                value["files"][0]["justification"] = json!("Cohesive.");
            }
            6 => {
                value["files"][0]["line_count"] = json!(1501);
                value["files"][0]["content_kind"] = json!("declarative");
                value["files"][0]["justification"] = json!("Cohesive.");
            }
            _ => unreachable!(),
        }
        replace_report(&mut request, value);
        assert!(
            request.validate(&definition).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn engineering_review_records_honest_rework_without_pass_shape() {
    let (definition, mut request) = completion();
    let mut value = report(&request);
    value["verdict"] = json!("rework");
    value.as_object_mut().unwrap().remove("source_basis");
    value["assessments"] = json!([{"rule_id":"ENG-02","status":"violation","rationale":"Responsibility is split.","evidence_refs":["implementation-plan.md#task-1"]}]);
    value["findings"] = json!([{"id":"F-1","rule_id":"ENG-02","status":"open","evidence":"Responsibility is split."}]);
    value["files"] = json!([]);
    replace_report(&mut request, value);
    request.output.verdict = Some("rework".into());
    request.output.dispositions = vec!["engineering_review_rework".into()];
    request.revisit_phase_id = Some("slice-lightweight-contract-writer".into());
    assert!(request.validate(&definition).is_ok());
}

#[test]
fn full_review_chain_preserves_findings_and_grants_authority_only_after_plan_review() {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::FullDesignToExecution)
        .unwrap();
    let expected = [
        "slice-cross-cutting-reviewer",
        "slice-reconciliation-runner",
        "slice-implementation-spec-synthesizer",
        "slice-spec-readiness-checker",
        "slice-plan-builder",
        "slice-engineering-plan-review",
        "slice-human-decision-queue-manager",
        "slice-execution-runner",
    ];
    let ordinals = expected
        .iter()
        .map(|id| {
            definition
                .phases
                .iter()
                .find(|phase| phase.id == *id)
                .unwrap()
                .ordinal
        })
        .collect::<Vec<_>>();
    assert!(ordinals.windows(2).all(|pair| pair[0] < pair[1]));
    let execution = definition
        .phases
        .iter()
        .find(|phase| phase.id == "slice-execution-runner")
        .unwrap();
    assert!(execution.output_constraints.iter().any(|constraint| matches!(
        constraint,
        tect_domain::PipelineOutputConstraint::CodeAuthorization { required_plan_review_phase_id }
            if required_plan_review_phase_id == "slice-engineering-plan-review"
    )));

    let (mut definition, mut request) = completion();
    let phase = definition
        .phases
        .iter_mut()
        .find(|phase| phase.id == request.phase_id)
        .unwrap();
    let review_constraint = phase
        .output_constraints
        .iter()
        .find(|constraint| {
            matches!(
                constraint,
                tect_domain::PipelineOutputConstraint::EngineeringReview { .. }
            )
        })
        .cloned()
        .expect("lightweight review phase has an engineering constraint");
    phase.output_constraints = match review_constraint {
        tect_domain::PipelineOutputConstraint::EngineeringReview {
            stage,
            standards_resource_id,
            standards_resource_digest,
            artifact_name,
            success_verdicts,
            ..
        } => vec![tect_domain::PipelineOutputConstraint::EngineeringReview {
            stage,
            standards_resource_id,
            standards_resource_digest,
            artifact_name,
            success_verdicts,
            required_prior_review_phase_ids: vec!["slice-test-target-selector".into()],
            required_reconciliation_phase_id: None,
        }],
        _ => unreachable!(),
    };
    let mut baseline = report(&request);
    baseline["prior_finding_ids"] = json!(["ENG-F1"]);
    baseline["resolved_finding_ids"] = json!(["ENG-F1"]);
    replace_report(&mut request, baseline);
    assert!(request.validate(&definition).is_ok());

    let mut dropped = request.clone();
    let mut dropped_report = report(&dropped);
    dropped_report["resolved_finding_ids"] = json!([]);
    replace_report(&mut dropped, dropped_report);
    assert!(dropped.validate(&definition).is_err());

    let mut reconciled_with_new_finding = request.clone();
    let mut reconciled_report = report(&reconciled_with_new_finding);
    reconciled_report["resolved_finding_ids"] = json!(["ENG-F1", "ENG-F2"]);
    reconciled_report["findings"] = json!([{
        "id":"ENG-F2",
        "rule_id":"ENG-03",
        "status":"resolved",
        "resolution":"The plan review reconciled the newly discovered finding."
    }]);
    replace_report(&mut reconciled_with_new_finding, reconciled_report);
    assert!(reconciled_with_new_finding.validate(&definition).is_ok());

    let mut stale = request.clone();
    stale.consumed_outputs[0].digest = "stale-plan-approval".into();
    assert!(stale.validate(&definition).is_err());

    let mut expanded = request;
    let mut expanded_report = report(&expanded);
    expanded_report["scope_expansion_authority"] = json!("invented");
    replace_report(&mut expanded, expanded_report);
    assert!(expanded.validate(&definition).is_err());
}

#[test]
fn implementation_authority_rejects_missing_or_substituted_plan_review() {
    let (mut definition, mut request) = completion();
    let phase = &mut definition.phases[0];
    phase.id = "slice-execution-runner".into();
    phase.output_constraints = vec![tect_domain::PipelineOutputConstraint::CodeAuthorization {
        required_plan_review_phase_id: "slice-engineering-plan-review".into(),
    }];
    request.phase_id = phase.id.clone();
    request.output.artifacts.clear();
    request.output.reviewer_context = None;
    request.output.resource_reads.clear();
    request.consumed_outputs.clear();
    assert!(request.validate(&definition).is_err());
    request.consumed_outputs.push(PipelineConsumedOutput {
        phase_id: "substituted-review".into(),
        output_revision: 1,
        digest: "substituted".into(),
    });
    assert!(request.validate(&definition).is_err());
}

#[test]
fn engineering_review_rejects_empty_or_dishonest_nonpass() {
    for mutation in 0..2 {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        value["verdict"] = json!("rework");
        value.as_object_mut().unwrap().remove("source_basis");
        value["files"] = json!([]);
        if mutation == 0 {
            value["assessments"] = json!([]);
            value["findings"] = json!([]);
        } else {
            value["assessments"] = json!([{"rule_id":"ENG-02","status":"satisfied","rationale":"The boundary is cohesive.","evidence_refs":["implementation-plan.md#task-1"]}]);
            value["findings"] = json!([{"id":"F-1","rule_id":"ENG-02","status":"open","evidence":"A missing basis must be resolved."}]);
        }
        replace_report(&mut request, value);
        request.output.verdict = Some("rework".into());
        request.output.dispositions = vec!["engineering_review_rework".into()];
        request.revisit_phase_id = Some("slice-lightweight-contract-writer".into());
        assert!(
            request.validate(&definition).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn engineering_review_records_honest_partial_blocked_report() {
    let (definition, mut request) = completion();
    let mut value = report(&request);
    value["verdict"] = json!("blocked");
    value.as_object_mut().unwrap().remove("source_basis");
    value["assessments"] = json!([{"rule_id":"ENG-03","status":"unassessed","rationale":"The dependency source is unavailable.","evidence_refs":["missing:dependency-source"]}]);
    value["findings"] = json!([]);
    value["files"] = json!([]);
    replace_report(&mut request, value);
    request.output.verdict = Some("blocked".into());
    request.output.dispositions = vec!["engineering_review_blocked".into()];
    request.outcome = PipelinePhaseOutcome::Blocked;
    request.transition = PipelineTransition::Block;
    assert!(request.validate(&definition).is_ok());
}

#[test]
fn implementation_review_requires_observed_counts_and_sha256() {
    let (mut definition, mut request) = completion();
    let phase = definition
        .phases
        .iter_mut()
        .find(|phase| phase.id == request.phase_id)
        .unwrap();
    if let tect_domain::PipelineOutputConstraint::EngineeringReview { stage, .. } = phase
        .output_constraints
        .iter_mut()
        .find(|constraint| {
            matches!(
                constraint,
                tect_domain::PipelineOutputConstraint::EngineeringReview { .. }
            )
        })
        .unwrap()
    {
        *stage = "implementation".into();
    }
    let mut value = report(&request);
    value["stage"] = json!("implementation");
    value["files"][0]["count_basis"] = json!("observed");
    value["files"][0]["content_digest"] = json!("a".repeat(64));
    replace_report(&mut request, value);
    assert!(request.validate(&definition).is_ok());
    let mut value = report(&request);
    value["files"][0]
        .as_object_mut()
        .unwrap()
        .remove("content_digest");
    replace_report(&mut request, value);
    assert!(request.validate(&definition).is_err());
}

#[test]
fn engineering_review_paths_have_safe_indexed_diagnostics() {
    for path in [
        "/private/secret-marker.txt",
        "../secret-marker.txt",
        "src/../secret-marker.txt",
        "src\\secret-marker.txt",
        "",
    ] {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        value["summary"] = json!("distinct-body-secret-marker");
        let mut invalid = value["files"][0].clone();
        invalid["path"] = json!(path);
        value["files"].as_array_mut().unwrap().push(invalid);
        replace_report(&mut request, value);
        let error = request.validate(&definition).unwrap_err();
        assert_eq!(error.code(), "invalid_arguments");
        let refusal = error.refusal().unwrap();
        assert_eq!(refusal.code, tect_domain::RefusalCode::InputSchemaInvalid);
        assert_eq!(refusal.rule.as_deref(), Some("ENG-REVIEW-FILE-PATH-01"));
        assert_eq!(
            refusal.path.as_deref(),
            Some("engineering-review.json/files/1/path")
        );
        assert_eq!(refusal.actual.as_deref(), Some("unsafe path"));
        assert!(refusal.is_complete_pipeline_refusal());
        let diagnostic = serde_json::to_string(&refusal).unwrap();
        assert!(!diagnostic.contains("secret-marker"));
        assert!(!diagnostic.contains("distinct-body-secret-marker"));
        assert!(diagnostic.len() < 600);
    }
    {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        value["summary"] = json!("duplicate-body-secret-marker");
        let mut duplicate = value["files"][0].clone();
        duplicate["path"] = json!("../duplicate-path-secret-marker.txt");
        value["files"]
            .as_array_mut()
            .unwrap()
            .push(duplicate.clone());
        value["files"].as_array_mut().unwrap().push(duplicate);
        replace_report(&mut request, value);
        let error = request.validate(&definition).unwrap_err();
        assert_eq!(error.code(), "invalid_arguments");
        let refusal = error.refusal().unwrap();
        assert_eq!(refusal.code, tect_domain::RefusalCode::InputSchemaInvalid);
        assert_eq!(refusal.rule.as_deref(), Some("ENG-REVIEW-FILE-PATH-01"));
        assert_eq!(
            refusal.path.as_deref(),
            Some("engineering-review.json/files/1/path")
        );
        let diagnostic = serde_json::to_string(&refusal).unwrap();
        assert!(!diagnostic.contains("duplicate-path-secret-marker"));
        assert!(!diagnostic.contains("duplicate-body-secret-marker"));
    }
    {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        value["summary"] = json!("invalid-field-body-secret-marker");
        value["files"][0]["path"] = json!("../invalid-field-secret-marker.txt");
        value["files"][0]["responsibility"] = json!(" ");
        replace_report(&mut request, value);
        let error = request.validate(&definition).unwrap_err();
        assert_eq!(error.code(), "invalid_arguments");
        let refusal = error.refusal().unwrap();
        assert_eq!(refusal.code, tect_domain::RefusalCode::InputSchemaInvalid);
        assert_eq!(refusal.rule.as_deref(), Some("ENG-REVIEW-FILE-PATH-01"));
        assert_eq!(
            refusal.path.as_deref(),
            Some("engineering-review.json/files/0/path")
        );
        let diagnostic = serde_json::to_string(&refusal).unwrap();
        assert!(!diagnostic.contains("invalid-field-secret-marker"));
        assert!(!diagnostic.contains("invalid-field-body-secret-marker"));
    }
    for path in ["src/service.rs", "README.md", "src/é🙂.rs"] {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        value["files"][0]["path"] = json!(path);
        replace_report(&mut request, value);
        assert!(request.validate(&definition).is_ok());
    }
}
