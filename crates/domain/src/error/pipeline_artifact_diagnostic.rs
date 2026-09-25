use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineArtifactViolation {
    pub code: String,
    pub path: String,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineArtifactDiagnostic {
    pub code: String,
    pub phase: String,
    pub artifact: String,
    pub violations: Vec<PipelineArtifactViolation>,
    pub truncated: bool,
    pub omitted_violation_count: usize,
    pub retryable: bool,
    pub recovery_action: String,
}

pub const MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VIOLATIONS: usize = 24;
const MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_CODE_BYTES: usize = 64;
const MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_IDENTITY_BYTES: usize = 128;
const MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_PATH_BYTES: usize = 192;
const MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VALUE_BYTES: usize = 192;
const MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_RECOVERY_BYTES: usize = 512;

fn bounded_text(value: String, maximum: usize) -> (String, bool) {
    if value.len() <= maximum {
        return (value, false);
    }
    const SUFFIX: &str = "...[truncated]";
    let mut end = maximum.saturating_sub(SUFFIX.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (format!("{}{SUFFIX}", &value[..end]), true)
}

impl PipelineArtifactDiagnostic {
    pub fn bounded(
        code: String,
        phase: String,
        artifact: String,
        violations: Vec<PipelineArtifactViolation>,
        retryable: bool,
        recovery_action: String,
    ) -> Self {
        Self::bounded_with_omitted(
            code,
            phase,
            artifact,
            violations,
            0,
            false,
            retryable,
            recovery_action,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bounded_with_omitted(
        code: String,
        phase: String,
        artifact: String,
        violations: Vec<PipelineArtifactViolation>,
        previously_omitted_violation_count: usize,
        previously_truncated: bool,
        retryable: bool,
        recovery_action: String,
    ) -> Self {
        let violation_count = violations.len();
        let (code, code_truncated) =
            bounded_text(code, MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_CODE_BYTES);
        let (phase, phase_truncated) =
            bounded_text(phase, MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_IDENTITY_BYTES);
        let (artifact, artifact_truncated) =
            bounded_text(artifact, MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_IDENTITY_BYTES);
        let mut text_truncated =
            previously_truncated || code_truncated || phase_truncated || artifact_truncated;
        let violations = violations
            .into_iter()
            .take(MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VIOLATIONS)
            .map(|violation| {
                let (code, code_truncated) =
                    bounded_text(violation.code, MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_CODE_BYTES);
                let (path, path_truncated) =
                    bounded_text(violation.path, MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_PATH_BYTES);
                let (expected, expected_truncated) =
                    violation.expected.map_or((None, false), |value| {
                        let (value, truncated) =
                            bounded_text(value, MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VALUE_BYTES);
                        (Some(value), truncated)
                    });
                let (actual, actual_truncated) = violation.actual.map_or((None, false), |value| {
                    let (value, truncated) =
                        bounded_text(value, MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VALUE_BYTES);
                    (Some(value), truncated)
                });
                text_truncated |=
                    code_truncated || path_truncated || expected_truncated || actual_truncated;
                PipelineArtifactViolation {
                    code,
                    path,
                    expected,
                    actual,
                }
            })
            .collect::<Vec<_>>();
        let omitted_violation_count = previously_omitted_violation_count
            .saturating_add(violation_count.saturating_sub(violations.len()));
        let (recovery_action, recovery_truncated) = bounded_text(
            recovery_action,
            MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_RECOVERY_BYTES,
        );
        Self {
            code,
            phase,
            artifact,
            violations,
            truncated: text_truncated || recovery_truncated || omitted_violation_count > 0,
            omitted_violation_count,
            retryable,
            recovery_action,
        }
    }
}
