use super::*;

pub(super) fn validate_verdict(
    report: &EngineeringReviewReport,
    artifact_index: usize,
    _stage: &str,
    pass: bool,
    _expected_report_verdict: &ReviewVerdict,
    requires_finding_lineage: bool,
) -> Result<()> {
    let fail = |rule, pointer: &str, expected: &str, actual: &str| {
        report_refusal(artifact_index, rule, pointer, expected, actual)
    };

    if pass {
        let assessed = report
            .assessments
            .iter()
            .map(|value| value.rule_id.as_str())
            .collect::<BTreeSet<_>>();
        if report.source_basis.is_none() {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-PASS-SOURCE",
                "/source_basis",
                "source basis for pass",
                "missing",
            ));
        }
        if assessed != RULES.into_iter().collect() {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-PASS-COVERAGE",
                "/assessments",
                "all ten ENG rules assessed",
                "incomplete coverage",
            ));
        }
        if let Some(index) = report.assessments.iter().position(|value| {
            matches!(
                value.status,
                AssessmentStatus::Violation | AssessmentStatus::Unassessed
            )
        }) {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-PASS-ASSESSMENT",
                &format!("/assessments/{index}/status"),
                "satisfied or not_applicable for pass",
                "problem assessment",
            ));
        }
        if let Some(index) = report
            .findings
            .iter()
            .position(|value| value.status == FindingStatus::Open)
        {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-PASS-OPEN-FINDING",
                &format!("/findings/{index}/status"),
                "no open findings for pass",
                "open",
            ));
        }
        if report.stage != ReviewStage::Specification && report.files.is_empty() {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-PASS-FILES",
                "/files",
                "at least one file for plan or implementation pass",
                "count=0",
            ));
        }
        if requires_finding_lineage {
            let prior = report.prior_finding_ids.as_ref().ok_or_else(|| {
                fail(
                    "WP6-ENGINEERING-REPORT-LINEAGE-PRIOR",
                    "/prior_finding_ids",
                    "prior finding IDs when prior review required",
                    "missing",
                )
            })?;
            let resolved = report.resolved_finding_ids.as_ref().ok_or_else(|| {
                fail(
                    "WP6-ENGINEERING-REPORT-LINEAGE-RESOLVED",
                    "/resolved_finding_ids",
                    "resolved finding IDs when prior review required",
                    "missing",
                )
            })?;
            match report.stage {
                ReviewStage::Specification if prior != resolved => {
                    return Err(fail(
                        "WP6-ENGINEERING-REPORT-LINEAGE-SPECIFICATION",
                        "/resolved_finding_ids",
                        "ordered equality with prior_finding_ids",
                        "mismatched",
                    ));
                }
                ReviewStage::Plan
                    if prior
                        .iter()
                        .any(|finding_id| !resolved.contains(finding_id)) =>
                {
                    return Err(fail(
                        "WP6-ENGINEERING-REPORT-LINEAGE-PLAN",
                        "/resolved_finding_ids",
                        "all prior finding IDs resolved",
                        "prior ID absent",
                    ));
                }
                _ => {}
            }
        }
    } else {
        if report.assessments.is_empty() {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-NONPASS-ASSESSMENTS",
                "/assessments",
                "at least one assessment",
                "count=0",
            ));
        }
        if let Some(index) = report.findings.iter().position(|finding| {
            finding.status == FindingStatus::Open
                && !report.assessments.iter().any(|assessment| {
                    assessment.rule_id == finding.rule_id
                        && matches!(
                            assessment.status,
                            AssessmentStatus::Violation | AssessmentStatus::Unassessed
                        )
                })
        }) {
            return Err(fail(
                "WP6-ENGINEERING-REPORT-OPEN-FINDING-ASSESSMENT",
                &format!("/findings/{index}/rule_id"),
                "matching violation or unassessed assessment",
                "absent",
            ));
        }
        let has_problem_assessment = report.assessments.iter().any(|assessment| {
            matches!(
                assessment.status,
                AssessmentStatus::Violation | AssessmentStatus::Unassessed
            )
        });
        let has_unassessed = report
            .assessments
            .iter()
            .any(|assessment| assessment.status == AssessmentStatus::Unassessed);
        let has_open_finding = report
            .findings
            .iter()
            .any(|finding| finding.status == FindingStatus::Open);
        match report.verdict {
            ReviewVerdict::Rework if !has_problem_assessment && !has_open_finding => {
                return Err(fail(
                    "WP6-ENGINEERING-REPORT-REWORK-BASIS",
                    "/verdict",
                    "problem assessment or open finding for rework",
                    "both absent",
                ));
            }
            ReviewVerdict::Blocked if !has_unassessed && !has_open_finding => {
                return Err(fail(
                    "WP6-ENGINEERING-REPORT-BLOCKED-BASIS",
                    "/verdict",
                    "unassessed assessment or open finding for blocked",
                    "both absent",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}
