use crate::*;
use serde::Deserialize;
use std::{collections::BTreeSet, path::Component};

mod constraints;
mod report;
pub(crate) use constraints::{validate_completion_constraints, validate_definition_constraints};
use report::validate_report;

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

fn engineering_refusal(
    code: RefusalCode,
    rule: &'static str,
    path: &str,
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> Error {
    let (next_action, required) = if code == RefusalCode::InputSchemaInvalid {
        ("correct_input_and_retry", "schema_valid_input")
    } else {
        ("correct_output", "output")
    };
    Error::Refused(Box::new(
        Refusal::new(code)
            .with_message(code.message())
            .with_rule(rule)
            .with_path(path)
            .with_expected(expected)
            .with_actual(actual)
            .with_next_action(next_action)
            .with_required(required),
    ))
}

fn report_refusal(
    index: usize,
    rule: &'static str,
    pointer: &str,
    expected: impl AsRef<str>,
    actual: impl Into<String>,
) -> Error {
    engineering_refusal(
        RefusalCode::InvalidOutput,
        rule,
        &format!("output.artifacts[{index}].body"),
        format!("decoded JSON {pointer}: {}", expected.as_ref()),
        actual,
    )
}

fn duplicate_index<T: Ord>(values: impl IntoIterator<Item = T>) -> Option<usize> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .enumerate()
        .find_map(|(index, value)| (!seen.insert(value)).then_some(index))
}

fn unsafe_file_path(value: &str) -> bool {
    blank(value)
        || value.contains('\\')
        || std::path::Path::new(value)
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
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

#[cfg(test)]
mod tests;
