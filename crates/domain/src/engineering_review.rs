use crate::*;
use serde::Deserialize;
use std::{collections::BTreeSet, path::Component};

const RULES: [&str; 10] = [
    "ENG-01", "ENG-02", "ENG-03", "ENG-04", "ENG-05", "ENG-06", "ENG-07", "ENG-08", "ENG-09",
    "ENG-10",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineeringReviewReport {
    stage: ReviewStage,
    rules_digest: String,
    verdict: ReviewVerdict,
    reviewed_outputs: Vec<PipelineConsumedOutput>,
    source_basis: Option<String>,
    prior_finding_ids: Option<Vec<String>>,
    resolved_finding_ids: Option<Vec<String>>,
    assessments: Vec<RuleAssessment>,
    findings: Vec<EngineeringFinding>,
    files: Vec<EngineeringFile>,
    summary: String,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReviewStage {
    Specification,
    Plan,
    Implementation,
}

impl ReviewStage {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Specification => "specification",
            Self::Plan => "plan",
            Self::Implementation => "implementation",
        }
    }
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ReviewVerdict {
    Pass,
    Rework,
    Blocked,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleAssessment {
    rule_id: String,
    status: AssessmentStatus,
    rationale: String,
    evidence_refs: Vec<String>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AssessmentStatus {
    Satisfied,
    NotApplicable,
    Violation,
    Unassessed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineeringFinding {
    id: String,
    rule_id: String,
    status: FindingStatus,
    evidence: Option<String>,
    resolution: Option<String>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum FindingStatus {
    Open,
    Resolved,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EngineeringFile {
    path: String,
    content_kind: ContentKind,
    line_count: u64,
    count_basis: CountBasis,
    content_digest: Option<String>,
    responsibility: String,
    justification: Option<String>,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ContentKind {
    Behavioral,
    Mixed,
    Declarative,
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CountBasis {
    Estimate,
    Observed,
}

pub(crate) fn validate_definition_constraints(
    definition: &PipelineDefinitionSnapshot,
) -> Result<()> {
    for phase in &definition.phases {
        for constraint in &phase.output_constraints {
            match constraint {
                PipelineOutputConstraint::EngineeringReview {
                    standards_resource_id,
                    standards_resource_digest,
                    artifact_name,
                    required_prior_review_phase_ids,
                    required_reconciliation_phase_id,
                    ..
                } => {
                    let standards = phase.resources.iter().any(|resource| {
                        resource.id == *standards_resource_id
                            && resource.version == "1.0.0"
                            && resource.digest == *standards_resource_digest
                    });
                    let review_skill = phase.resources.iter().any(|resource| {
                        resource.id == "tect:engineering-review" && resource.version == "1.0.0"
                    });
                    let report = phase.required_artifacts.iter().any(|artifact| {
                        artifact.name_pattern == *artifact_name
                            && artifact.media_type == "application/json"
                            && artifact.required
                            && artifact.minimum_matches == 1
                            && artifact.schema_resource_id.as_deref()
                                == Some("tect:engineering-review-schema")
                    });
                    if !standards || !review_skill || !report {
                        return Err(Error::InvalidArguments);
                    }
                    for prior_id in required_prior_review_phase_ids {
                        let prior = definition
                            .phases
                            .iter()
                            .find(|candidate| {
                                candidate.id == *prior_id && candidate.ordinal < phase.ordinal
                            })
                            .ok_or(Error::InvalidArguments)?;
                        if !prior.output_constraints.iter().any(|candidate| {
                            matches!(
                                candidate,
                                PipelineOutputConstraint::EngineeringReview { .. }
                            )
                        }) {
                            return Err(Error::InvalidArguments);
                        }
                    }
                    if let Some(reconciliation_id) = required_reconciliation_phase_id {
                        let reconciliation = definition
                            .phases
                            .iter()
                            .find(|candidate| {
                                candidate.id == *reconciliation_id
                                    && candidate.ordinal < phase.ordinal
                            })
                            .ok_or(Error::InvalidArguments)?;
                        let required = [
                            "engineering_finding_ids",
                            "resolved_engineering_finding_ids",
                            "deferred_engineering_finding_ids",
                            "unresolved_engineering_finding_count",
                        ];
                        if required.iter().any(|field| {
                            !reconciliation
                                .required_fields
                                .iter()
                                .any(|value| value == field)
                        }) {
                            return Err(Error::InvalidArguments);
                        }
                    }
                }
                PipelineOutputConstraint::CodeAuthorization {
                    required_plan_review_phase_id,
                } => {
                    let prior = definition
                        .phases
                        .iter()
                        .find(|candidate| {
                            candidate.id == *required_plan_review_phase_id
                                && candidate.ordinal < phase.ordinal
                        })
                        .ok_or(Error::InvalidArguments)?;
                    if !prior.output_constraints.iter().any(|candidate| {
                        matches!(candidate, PipelineOutputConstraint::EngineeringReview { stage, .. } if stage == "plan")
                    }) {
                        return Err(Error::InvalidArguments);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_completion_constraints(
    request: &CompletePipelinePhase,
    _definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<()> {
    for constraint in &phase.output_constraints {
        match constraint {
            PipelineOutputConstraint::EngineeringReview {
                stage,
                standards_resource_digest,
                artifact_name,
                success_verdicts,
                required_prior_review_phase_ids,
                ..
            } => {
                let artifact = request
                    .output
                    .artifacts
                    .iter()
                    .find(|artifact| artifact.name == *artifact_name)
                    .ok_or(Error::InvalidArguments)?;
                let report: EngineeringReviewReport =
                    serde_json::from_str(&artifact.body).map_err(|_| Error::InvalidArguments)?;
                validate_report(
                    &report,
                    request,
                    stage,
                    standards_resource_digest,
                    success_verdicts,
                    !required_prior_review_phase_ids.is_empty(),
                )?;
                if report.verdict == ReviewVerdict::Pass
                    && required_prior_review_phase_ids.iter().any(|required| {
                        !request
                            .consumed_outputs
                            .iter()
                            .any(|output| output.phase_id == *required)
                    })
                {
                    return Err(Error::InvalidArguments);
                }
            }
            PipelineOutputConstraint::CodeAuthorization {
                required_plan_review_phase_id,
            } => {
                if !request
                    .consumed_outputs
                    .iter()
                    .any(|output| output.phase_id == *required_plan_review_phase_id)
                {
                    return Err(Error::InvalidArguments);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_report(
    report: &EngineeringReviewReport,
    request: &CompletePipelinePhase,
    stage: &str,
    rules_digest: &str,
    success_verdicts: &[String],
    requires_finding_lineage: bool,
) -> Result<()> {
    let output_verdict = request
        .output
        .verdict
        .as_deref()
        .ok_or(Error::InvalidArguments)?;
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
    if report.stage.as_str() != stage
        || report.rules_digest != rules_digest
        || report.verdict != expected_report_verdict
        || report.reviewed_outputs != request.consumed_outputs
        || blank(&report.summary)
        || report
            .source_basis
            .as_ref()
            .is_some_and(|value| blank(value))
        || report
            .prior_finding_ids
            .as_ref()
            .is_some_and(|values| values.iter().any(|value| blank(value)) || !unique(values.iter()))
        || report
            .resolved_finding_ids
            .as_ref()
            .is_some_and(|values| values.iter().any(|value| blank(value)) || !unique(values.iter()))
        || report.assessments.iter().any(invalid_assessment)
        || report.findings.iter().any(invalid_finding)
        || report.files.iter().any(invalid_file)
        || !unique(
            report
                .reviewed_outputs
                .iter()
                .map(|value| (&value.phase_id, value.output_revision, &value.digest)),
        )
        || !unique(report.assessments.iter().map(|value| &value.rule_id))
        || !unique(report.findings.iter().map(|value| &value.id))
        || !unique(report.files.iter().map(|value| &value.path))
    {
        return Err(Error::InvalidArguments);
    }
    if pass {
        let assessed = report
            .assessments
            .iter()
            .map(|value| value.rule_id.as_str())
            .collect::<BTreeSet<_>>();
        if report.source_basis.is_none()
            || assessed != RULES.into_iter().collect()
            || report.assessments.iter().any(|value| {
                matches!(
                    value.status,
                    AssessmentStatus::Violation | AssessmentStatus::Unassessed
                )
            })
            || report
                .findings
                .iter()
                .any(|value| value.status == FindingStatus::Open)
            || report.stage != ReviewStage::Specification && report.files.is_empty()
        {
            return Err(Error::InvalidArguments);
        }
        if requires_finding_lineage
            && report.stage == ReviewStage::Specification
            && (report.prior_finding_ids.is_none()
                || report.resolved_finding_ids.is_none()
                || report.prior_finding_ids != report.resolved_finding_ids)
        {
            return Err(Error::InvalidArguments);
        }
    } else {
        if report.assessments.is_empty()
            || report.findings.iter().any(|finding| {
                finding.status == FindingStatus::Open
                    && !report.assessments.iter().any(|assessment| {
                        assessment.rule_id == finding.rule_id
                            && matches!(
                                assessment.status,
                                AssessmentStatus::Violation | AssessmentStatus::Unassessed
                            )
                    })
            })
        {
            return Err(Error::InvalidArguments);
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
                return Err(Error::InvalidArguments);
            }
            ReviewVerdict::Blocked if !has_unassessed && !has_open_finding => {
                return Err(Error::InvalidArguments);
            }
            _ => {}
        }
    }
    if report.stage == ReviewStage::Implementation
        && report.files.iter().any(|file| {
            file.count_basis != CountBasis::Observed
                || file
                    .content_digest
                    .as_ref()
                    .is_none_or(|value| !sha256(value))
        })
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn invalid_assessment(value: &RuleAssessment) -> bool {
    !RULES.contains(&value.rule_id.as_str())
        || blank(&value.rationale)
        || value.evidence_refs.iter().any(|reference| blank(reference))
        || value.evidence_refs.is_empty()
}

fn invalid_finding(value: &EngineeringFinding) -> bool {
    blank(&value.id)
        || !RULES.contains(&value.rule_id.as_str())
        || value.evidence.as_ref().is_some_and(|text| blank(text))
        || value.resolution.as_ref().is_some_and(|text| blank(text))
        || value.status == FindingStatus::Open && value.evidence.is_none()
        || value.status == FindingStatus::Resolved && value.resolution.is_none()
}

fn invalid_file(value: &EngineeringFile) -> bool {
    let path = std::path::Path::new(&value.path);
    blank(&value.path)
        || value.path.contains('\\')
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        || blank(&value.responsibility)
        || value.justification.as_ref().is_some_and(|text| blank(text))
        || value.line_count > 1500
        || value.line_count > 1000 && value.content_kind != ContentKind::Declarative
        || value.line_count > 500 && value.justification.is_none()
        || value
            .content_digest
            .as_ref()
            .is_some_and(|value| !sha256(value))
}

fn unique<T: Ord>(values: impl IntoIterator<Item = T>) -> bool {
    let values = values.into_iter().collect::<Vec<_>>();
    values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn blank(value: &str) -> bool {
    value.trim().is_empty()
}
fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
