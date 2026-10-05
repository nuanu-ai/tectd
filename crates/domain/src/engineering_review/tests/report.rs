use super::*;

#[test]
fn engineering_report_accepts_valid_stages_and_preserves_thresholds() {
    for stage in ["specification", "plan", "implementation"] {
        let mut value = report();
        value["stage"] = json!(stage);
        assert!(check(value, &request(), stage, true).is_ok());
    }
    for (count, kind, justification) in [
        (500, "behavioral", Value::Null),
        (501, "behavioral", json!("bounded")),
        (1000, "behavioral", json!("bounded")),
        (1001, "declarative", json!("generated")),
        (1500, "declarative", json!("generated")),
    ] {
        let mut value = report();
        value["files"][0]["line_count"] = json!(count);
        value["files"][0]["content_kind"] = json!(kind);
        value["files"][0]["justification"] = justification;
        assert!(check(value, &request(), "specification", false).is_ok());
    }
    let mut value = report();
    value["files"] = json!([]);
    assert!(check(value, &request(), "specification", false).is_ok());
}

#[test]
fn engineering_report_header_and_indexed_fields_have_precise_output_refusals() {
    let cases = [
        ("/stage", json!("plan"), "STAGE"),
        ("/rules_digest", json!("wrong"), "RULES-DIGEST"),
        ("/verdict", json!("rework"), "VERDICT"),
        (
            "/reviewed_outputs",
            json!([{"phase_id":"prior","output_revision":1,"digest":"d"}]),
            "REVIEWED-OUTPUTS",
        ),
        ("/summary", json!(" "), "SUMMARY"),
        ("/source_basis", json!(" "), "SOURCE-BASIS"),
        ("/prior_finding_ids", json!([""]), "PRIOR-ID"),
        (
            "/prior_finding_ids",
            json!(["F", "F"]),
            "PRIOR-ID-DUPLICATE",
        ),
        ("/resolved_finding_ids", json!([""]), "RESOLVED-ID"),
        (
            "/resolved_finding_ids",
            json!(["F", "F"]),
            "RESOLVED-ID-DUPLICATE",
        ),
        ("/assessments/0/rule_id", json!("bad"), "ASSESSMENT-RULE"),
        (
            "/assessments/0/rationale",
            json!(""),
            "ASSESSMENT-RATIONALE",
        ),
        (
            "/assessments/0/evidence_refs",
            json!([""]),
            "ASSESSMENT-EVIDENCE",
        ),
        (
            "/assessments/0/evidence_refs",
            json!([]),
            "ASSESSMENT-EVIDENCE-MISSING",
        ),
        ("/files/0/responsibility", json!(""), "FILE-RESPONSIBILITY"),
        ("/files/0/justification", json!(" "), "FILE-JUSTIFICATION"),
        ("/files/0/line_count", json!(1501), "FILE-MAX-LINES"),
        ("/files/0/line_count", json!(1001), "FILE-DECLARATIVE"),
        ("/files/0/line_count", json!(501), "FILE-LINE-JUSTIFICATION"),
        ("/files/0/content_digest", json!("INVALID"), "FILE-DIGEST"),
    ];
    for (pointer, changed, suffix) in cases {
        let mut value = report();
        *value.pointer_mut(pointer).unwrap() = changed;
        let error = check(value, &request(), "specification", false).unwrap_err();
        let refusal = error.refusal().unwrap();
        assert_eq!(refusal.code, RefusalCode::InvalidOutput, "{suffix}");
        assert_eq!(
            refusal.rule.as_deref(),
            Some(format!("WP6-ENGINEERING-REPORT-{suffix}").as_str())
        );
        assert_eq!(refusal.path.as_deref(), Some("output.artifacts[2].body"));
        // Array predicates report the actual failing item, even when the mutation replaced its list.
        assert!(
            refusal
                .expected
                .as_deref()
                .unwrap()
                .contains("decoded JSON /")
        );
    }
    let mut value = report();
    value["stage"] = json!("plan");
    value["rules_digest"] = json!("wrong");
    assert_rule(
        check(value, &request(), "specification", false).unwrap_err(),
        "WP6-ENGINEERING-REPORT-STAGE",
        "/stage",
    );
}

#[test]
fn engineering_report_finding_and_duplicate_branches_remain_distinct() {
    for (field, changed, suffix) in [
        ("id", json!(""), "FINDING-ID"),
        ("rule_id", json!("bad"), "FINDING-RULE"),
        ("evidence", json!(" "), "FINDING-EVIDENCE"),
        ("resolution", json!(" "), "FINDING-RESOLUTION"),
        ("resolution", Value::Null, "FINDING-RESOLVED-RESOLUTION"),
    ] {
        let mut value = report();
        value["findings"] = json!([finding()]);
        value["findings"][0][field] = changed;
        assert_rule(
            check(value, &request(), "specification", false).unwrap_err(),
            &format!("WP6-ENGINEERING-REPORT-{suffix}"),
            &format!("/findings/0/{field}"),
        );
    }
    let mut value = report();
    let mut open = finding();
    open["status"] = json!("open");
    open["evidence"] = Value::Null;
    value["findings"] = json!([open]);
    assert_rule(
        check(value, &request(), "specification", false).unwrap_err(),
        "WP6-ENGINEERING-REPORT-FINDING-OPEN-EVIDENCE",
        "/findings/0/evidence",
    );
    for (field, entries, suffix, pointer) in [
        (
            "assessments",
            json!([
                report()["assessments"][0].clone(),
                report()["assessments"][0].clone()
            ]),
            "ASSESSMENT-DUPLICATE",
            "/assessments/1/rule_id",
        ),
        (
            "findings",
            json!([finding(), finding()]),
            "FINDING-DUPLICATE",
            "/findings/1/id",
        ),
        (
            "files",
            json!([file(), file()]),
            "FILE-DUPLICATE",
            "/files/1/path",
        ),
    ] {
        let mut value = report();
        value[field] = entries;
        assert_rule(
            check(value, &request(), "specification", false).unwrap_err(),
            &format!("WP6-ENGINEERING-REPORT-{suffix}"),
            pointer,
        );
    }
    let outputs = json!([{"phase_id":"prior","output_revision":1,"digest":"d"},{"phase_id":"prior","output_revision":1,"digest":"d"}]);
    let mut req = request();
    req.consumed_outputs = serde_json::from_value(outputs.clone()).unwrap();
    let mut value = report();
    value["reviewed_outputs"] = outputs;
    assert_rule(
        check(value, &req, "specification", false).unwrap_err(),
        "WP6-ENGINEERING-REPORT-REVIEWED-OUTPUT-DUPLICATE",
        "/reviewed_outputs/1",
    );
}

#[test]
fn engineering_report_pass_lineage_and_implementation_requirements_preserve_order() {
    for (pointer, changed, suffix, stage, lineage) in [
        (
            "/source_basis",
            Value::Null,
            "PASS-SOURCE",
            "specification",
            false,
        ),
        (
            "/assessments",
            json!([]),
            "PASS-COVERAGE",
            "specification",
            false,
        ),
        (
            "/assessments/0/status",
            json!("violation"),
            "PASS-ASSESSMENT",
            "specification",
            false,
        ),
        ("/files", json!([]), "PASS-FILES", "plan", false),
        (
            "/prior_finding_ids",
            Value::Null,
            "LINEAGE-PRIOR",
            "specification",
            true,
        ),
        (
            "/resolved_finding_ids",
            Value::Null,
            "LINEAGE-RESOLVED",
            "specification",
            true,
        ),
        (
            "/files/0/count_basis",
            json!("estimate"),
            "IMPLEMENTATION-COUNT-BASIS",
            "implementation",
            false,
        ),
        (
            "/files/0/content_digest",
            Value::Null,
            "IMPLEMENTATION-DIGEST",
            "implementation",
            false,
        ),
    ] {
        let mut value = report();
        value["stage"] = json!(stage);
        *value.pointer_mut(pointer).unwrap() = changed;
        assert_rule(
            check(value, &request(), stage, lineage).unwrap_err(),
            &format!("WP6-ENGINEERING-REPORT-{suffix}"),
            pointer,
        );
    }
    let mut value = report();
    let mut open = finding();
    open["status"] = json!("open");
    value["findings"] = json!([open]);
    assert_rule(
        check(value, &request(), "specification", false).unwrap_err(),
        "WP6-ENGINEERING-REPORT-PASS-OPEN-FINDING",
        "/findings/0/status",
    );
    let mut value = report();
    value["prior_finding_ids"] = json!(["A", "B"]);
    value["resolved_finding_ids"] = json!(["B", "A"]);
    assert_rule(
        check(value.clone(), &request(), "specification", true).unwrap_err(),
        "WP6-ENGINEERING-REPORT-LINEAGE-SPECIFICATION",
        "/resolved_finding_ids",
    );
    value["stage"] = json!("plan");
    assert!(check(value.clone(), &request(), "plan", true).is_ok());
    value["resolved_finding_ids"] = json!(["A"]);
    assert_rule(
        check(value, &request(), "plan", true).unwrap_err(),
        "WP6-ENGINEERING-REPORT-LINEAGE-PLAN",
        "/resolved_finding_ids",
    );
}

#[test]
fn engineering_report_nonpass_basis_and_open_finding_assessment_are_enforced() {
    let mut req = request();
    req.output.verdict = Some("REWORK".into());
    let mut value = report();
    value["verdict"] = json!("rework");
    value["assessments"][0]["status"] = json!("violation");
    assert!(check(value.clone(), &req, "specification", false).is_ok());
    value["assessments"] = json!([]);
    assert_rule(
        check(value, &req, "specification", false).unwrap_err(),
        "WP6-ENGINEERING-REPORT-NONPASS-ASSESSMENTS",
        "/assessments",
    );
    let mut value = report();
    value["verdict"] = json!("rework");
    assert_rule(
        check(value.clone(), &req, "specification", false).unwrap_err(),
        "WP6-ENGINEERING-REPORT-REWORK-BASIS",
        "/verdict",
    );
    let mut open = finding();
    open["status"] = json!("open");
    value["findings"] = json!([open]);
    assert_rule(
        check(value, &req, "specification", false).unwrap_err(),
        "WP6-ENGINEERING-REPORT-OPEN-FINDING-ASSESSMENT",
        "/findings/0/rule_id",
    );
    req.outcome = PipelinePhaseOutcome::Blocked;
    let mut value = report();
    value["verdict"] = json!("blocked");
    assert_rule(
        check(value.clone(), &req, "specification", false).unwrap_err(),
        "WP6-ENGINEERING-REPORT-BLOCKED-BASIS",
        "/verdict",
    );
    value["assessments"][0]["status"] = json!("unassessed");
    assert!(check(value, &req, "specification", false).is_ok());
}

#[test]
fn engineering_report_unsafe_path_refusal_never_echoes_path() {
    let mut value = report();
    value["files"][0]["path"] = json!("../秘密/escape.rs");
    value["stage"] = json!("plan");
    let error = check(value, &request(), "specification", false).unwrap_err();
    let refusal = error.refusal().unwrap();
    assert_eq!(refusal.rule.as_deref(), Some("ENG-REVIEW-FILE-PATH-01"));
    assert_eq!(
        refusal.path.as_deref(),
        Some("engineering-review.json/files/0/path")
    );
    assert!(!serde_json::to_string(&error).unwrap().contains("秘密"));
}
