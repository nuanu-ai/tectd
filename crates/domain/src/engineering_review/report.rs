use super::*;

mod structure;
mod verdict;

pub(super) fn validate_report(
    report: &EngineeringReviewReport,
    request: &CompletePipelinePhase,
    artifact_index: usize,
    stage: &str,
    rules_digest: &str,
    success_verdicts: &[String],
    requires_finding_lineage: bool,
) -> Result<()> {
    let fail = |rule, pointer: &str, expected: &str, actual: &str| {
        report_refusal(artifact_index, rule, pointer, expected, actual)
    };
    let output_verdict = request.output.verdict.as_deref().ok_or_else(|| {
        engineering_refusal(
            RefusalCode::InvalidOutput,
            "WP6-ENGINEERING-REPORT-OUTPUT-VERDICT",
            "output.verdict",
            "phase verdict",
            "missing",
        )
    })?;
    let pass = success_verdicts
        .iter()
        .any(|verdict| verdict == output_verdict);
    let expected_report_verdict = if pass {
        ReviewVerdict::Pass
    } else if request.outcome != PipelinePhaseOutcome::Completed {
        ReviewVerdict::Blocked
    } else {
        ReviewVerdict::Rework
    };
    // Retain the established named safety refusal and its pre-header precedence.
    if let Some(index) = report
        .files
        .iter()
        .position(|file| unsafe_file_path(&file.path))
    {
        return Err(Error::PipelineRefused {
            source: Box::new(Error::InvalidArguments),
            refusal: Box::new(
                Refusal::new(RefusalCode::InputSchemaInvalid)
                    .with_message(RefusalCode::InputSchemaInvalid.message())
                    .with_rule("ENG-REVIEW-FILE-PATH-01")
                    .with_path(format!("engineering-review.json/files/{index}/path"))
                    .with_expected("safe repository-relative path")
                    .with_actual("unsafe path")
                    .with_next_action("correct_input_and_retry")
                    .with_required("schema_valid_input"),
            ),
        });
    }
    if report.stage.as_str() != stage {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-STAGE",
            "/stage",
            "phase review stage",
            "mismatched",
        ));
    }
    if report.rules_digest != rules_digest {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-RULES-DIGEST",
            "/rules_digest",
            "phase standards digest",
            "mismatched",
        ));
    }
    if report.verdict != expected_report_verdict {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-VERDICT",
            "/verdict",
            "verdict matching phase outcome and output verdict",
            "mismatched",
        ));
    }
    if report.reviewed_outputs != request.consumed_outputs {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-REVIEWED-OUTPUTS",
            "/reviewed_outputs",
            "ordered consumed_outputs including revisions and digests",
            "mismatched",
        ));
    }
    if blank(&report.summary) {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-SUMMARY",
            "/summary",
            "nonblank summary",
            "blank",
        ));
    }
    if report
        .source_basis
        .as_ref()
        .is_some_and(|value| blank(value))
    {
        return Err(fail(
            "WP6-ENGINEERING-REPORT-SOURCE-BASIS",
            "/source_basis",
            "nonblank source basis when present",
            "blank",
        ));
    }
    structure::validate_structure(report, artifact_index)?;
    verdict::validate_verdict(
        report,
        artifact_index,
        stage,
        pass,
        &expected_report_verdict,
        requires_finding_lineage,
    )?;
    if report.stage == ReviewStage::Implementation {
        for (index, file) in report.files.iter().enumerate() {
            if file.count_basis != CountBasis::Observed {
                return Err(fail(
                    "WP6-ENGINEERING-REPORT-IMPLEMENTATION-COUNT-BASIS",
                    &format!("/files/{index}/count_basis"),
                    "observed line count for implementation",
                    "estimate",
                ));
            }
            if file
                .content_digest
                .as_ref()
                .is_none_or(|value| !sha256(value))
            {
                return Err(fail(
                    "WP6-ENGINEERING-REPORT-IMPLEMENTATION-DIGEST",
                    &format!("/files/{index}/content_digest"),
                    "64 lowercase hexadecimal characters for implementation",
                    "missing or invalid format",
                ));
            }
        }
    }
    Ok(())
}
