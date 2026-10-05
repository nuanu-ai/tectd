use super::*;

pub(super) fn validate_structure(
    report: &EngineeringReviewReport,
    artifact_index: usize,
) -> Result<()> {
    let fail = |rule, pointer: &str, expected: &str, actual: &str| {
        report_refusal(artifact_index, rule, pointer, expected, actual)
    };
    for (field, values, blank_rule, duplicate_rule) in [
        (
            "prior_finding_ids",
            &report.prior_finding_ids,
            "WP6-ENGINEERING-REPORT-PRIOR-ID",
            "WP6-ENGINEERING-REPORT-PRIOR-ID-DUPLICATE",
        ),
        (
            "resolved_finding_ids",
            &report.resolved_finding_ids,
            "WP6-ENGINEERING-REPORT-RESOLVED-ID",
            "WP6-ENGINEERING-REPORT-RESOLVED-ID-DUPLICATE",
        ),
    ] {
        if let Some(values) = values {
            if let Some(index) = values.iter().position(|value| blank(value)) {
                return Err(fail(
                    blank_rule,
                    &format!("/{field}/{index}"),
                    "nonblank finding ID",
                    "blank",
                ));
            }
            if let Some(index) = duplicate_index(values.iter()) {
                return Err(fail(
                    duplicate_rule,
                    &format!("/{field}/{index}"),
                    "unique finding ID",
                    "duplicate",
                ));
            }
        }
    }
    for (index, value) in report.assessments.iter().enumerate() {
        for (invalid, field, rule, expected) in [
            (
                !RULES.contains(&value.rule_id.as_str()),
                "rule_id",
                "WP6-ENGINEERING-REPORT-ASSESSMENT-RULE",
                "ENG-01 through ENG-10",
            ),
            (
                blank(&value.rationale),
                "rationale",
                "WP6-ENGINEERING-REPORT-ASSESSMENT-RATIONALE",
                "nonblank rationale",
            ),
        ] {
            if invalid {
                return Err(fail(
                    rule,
                    &format!("/assessments/{index}/{field}"),
                    expected,
                    "invalid",
                ));
            }
        }
        if let Some(reference_index) = value
            .evidence_refs
            .iter()
            .position(|reference| blank(reference))
        {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-ASSESSMENT-EVIDENCE",
                &format!("/assessments/{index}/evidence_refs/{reference_index}"),
                "nonblank evidence reference",
                "blank",
            ));
        }
        if value.evidence_refs.is_empty() {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-ASSESSMENT-EVIDENCE-MISSING",
                &format!("/assessments/{index}/evidence_refs"),
                "at least one evidence reference",
                "count=0",
            ));
        }
    }
    for (index, value) in report.findings.iter().enumerate() {
        for (invalid, field, rule, expected, actual) in [
            (
                blank(&value.id),
                "id",
                "WP6-ENGINEERING-REPORT-FINDING-ID",
                "nonblank finding ID",
                "blank",
            ),
            (
                !RULES.contains(&value.rule_id.as_str()),
                "rule_id",
                "WP6-ENGINEERING-REPORT-FINDING-RULE",
                "ENG-01 through ENG-10",
                "invalid",
            ),
            (
                value.evidence.as_ref().is_some_and(|text| blank(text)),
                "evidence",
                "WP6-ENGINEERING-REPORT-FINDING-EVIDENCE",
                "nonblank evidence when present",
                "blank",
            ),
            (
                value.resolution.as_ref().is_some_and(|text| blank(text)),
                "resolution",
                "WP6-ENGINEERING-REPORT-FINDING-RESOLUTION",
                "nonblank resolution when present",
                "blank",
            ),
            (
                value.status == FindingStatus::Open && value.evidence.is_none(),
                "evidence",
                "WP6-ENGINEERING-REPORT-FINDING-OPEN-EVIDENCE",
                "evidence for open finding",
                "missing",
            ),
            (
                value.status == FindingStatus::Resolved && value.resolution.is_none(),
                "resolution",
                "WP6-ENGINEERING-REPORT-FINDING-RESOLVED-RESOLUTION",
                "resolution for resolved finding",
                "missing",
            ),
        ] {
            if invalid {
                return Err(fail(
                    rule,
                    &format!("/findings/{index}/{field}"),
                    expected,
                    actual,
                ));
            }
        }
    }
    for (index, value) in report.files.iter().enumerate() {
        for (invalid, field, rule, expected, actual) in [
            (
                blank(&value.responsibility),
                "responsibility",
                "WP6-ENGINEERING-REPORT-FILE-RESPONSIBILITY",
                "nonblank responsibility",
                "blank",
            ),
            (
                value.justification.as_ref().is_some_and(|text| blank(text)),
                "justification",
                "WP6-ENGINEERING-REPORT-FILE-JUSTIFICATION",
                "nonblank justification when present",
                "blank",
            ),
            (
                value.line_count > 1500,
                "line_count",
                "WP6-ENGINEERING-REPORT-FILE-MAX-LINES",
                "at most 1500 lines",
                "over threshold",
            ),
            (
                value.line_count > 1000 && value.content_kind != ContentKind::Declarative,
                "content_kind",
                "WP6-ENGINEERING-REPORT-FILE-DECLARATIVE",
                "declarative content above 1000 lines",
                "non-declarative",
            ),
            (
                value.line_count > 500 && value.justification.is_none(),
                "justification",
                "WP6-ENGINEERING-REPORT-FILE-LINE-JUSTIFICATION",
                "justification above 500 lines",
                "missing",
            ),
            (
                value
                    .content_digest
                    .as_ref()
                    .is_some_and(|value| !sha256(value)),
                "content_digest",
                "WP6-ENGINEERING-REPORT-FILE-DIGEST",
                "64 lowercase hexadecimal characters when present",
                "invalid format",
            ),
        ] {
            if invalid {
                return Err(fail(
                    rule,
                    &format!("/files/{index}/{field}"),
                    expected,
                    actual,
                ));
            }
        }
    }
    if let Some(index) = duplicate_index(
        report
            .reviewed_outputs
            .iter()
            .map(|value| (&value.phase_id, value.output_revision, &value.digest)),
    ) {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-REVIEWED-OUTPUT-DUPLICATE",
            &format!("/reviewed_outputs/{index}"),
            "unique phase/revision/digest tuple",
            "duplicate",
        ));
    }
    if let Some(index) = duplicate_index(report.assessments.iter().map(|value| &value.rule_id)) {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-ASSESSMENT-DUPLICATE",
            &format!("/assessments/{index}/rule_id"),
            "unique assessment rule",
            "duplicate",
        ));
    }
    if let Some(index) = duplicate_index(report.findings.iter().map(|value| &value.id)) {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-FINDING-DUPLICATE",
            &format!("/findings/{index}/id"),
            "unique finding ID",
            "duplicate",
        ));
    }
    if let Some(index) = duplicate_index(report.files.iter().map(|value| &value.path)) {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-FILE-DUPLICATE",
            &format!("/files/{index}/path"),
            "unique file path",
            "duplicate",
        ));
    }
    Ok(())
}
