use serde::{Deserialize, Serialize};
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Stable public errors deliberately contain no database or credential details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    Unauthorized,
    InvalidNativeSession,
    InvalidWorkspaceKey,
    SessionWorkspaceMismatch,
    SessionRevoked,
    WorkspaceNotOpen,
    Forbidden,
    InvalidArguments,
    InvalidSource,
    InvalidWorktreeSelection,
    NotFound,
    StorageUnavailable,
    InvalidConfiguration,
    TransportUnavailable,
    RequestTooLarge,
    StaleRevision,
    StaleContext,
    InputPending,
    ProgramIncomplete,
    InputConflict,
    TaskDirectoryUnbound,
    TaskDirectoryMismatch,
    SetupUnavailable,
    SetupExists,
    SetupIncomplete,
    SetupFileConflict,
    SetupAlreadyApplied,
    InternalInvariant,
    KnowledgeUnavailable,
    ContextChanged,
    NeedsContext,
    CapacityExceeded,
    KnowledgeLifecycleRequired,
    UnsupportedCompletionRequirement,
    KnowledgePayloadErased,
    InvalidPipelineArtifact(Box<PipelineArtifactDiagnostic>),
    /// Tool arguments failed to deserialize; the reason names the offending field.
    InvalidArgumentsDetail(Box<ArgumentDiagnostic>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgumentDiagnostic {
    pub reason: String,
}

/// Upper bound for a deserializer reason carried to the agent.
const ARGUMENT_REASON_LIMIT: usize = 600;

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

impl Error {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "unauthorized",
            Self::InvalidNativeSession => "invalid_native_session",
            Self::InvalidWorkspaceKey => "invalid_workspace_key",
            Self::SessionWorkspaceMismatch => "session_workspace_mismatch",
            Self::SessionRevoked => "session_revoked",
            Self::WorkspaceNotOpen => "workspace_not_open",
            Self::Forbidden => "forbidden",
            Self::InvalidArguments => "invalid_arguments",
            Self::InvalidSource => "invalid_source",
            Self::InvalidWorktreeSelection => "invalid_worktree_selection",
            Self::NotFound => "not_found",
            Self::StorageUnavailable => "storage_unavailable",
            Self::InvalidConfiguration => "invalid_configuration",
            Self::TransportUnavailable => "transport_unavailable",
            Self::RequestTooLarge => "request_too_large",
            Self::StaleRevision => "stale_revision",
            Self::StaleContext => "stale_context",
            Self::InputPending => "input_pending",
            Self::ProgramIncomplete => "program_incomplete",
            Self::InputConflict => "input_conflict",
            Self::TaskDirectoryUnbound => "task_directory_unbound",
            Self::TaskDirectoryMismatch => "task_directory_mismatch",
            Self::SetupUnavailable => "setup_unavailable",
            Self::SetupExists => "setup_exists",
            Self::SetupIncomplete => "setup_incomplete",
            Self::SetupFileConflict => "setup_file_conflict",
            Self::SetupAlreadyApplied => "setup_already_applied",
            Self::InternalInvariant => "internal_invariant",
            Self::KnowledgeUnavailable => "knowledge_unavailable",
            Self::ContextChanged => "context_changed",
            Self::NeedsContext => "needs_context",
            Self::CapacityExceeded => "capacity_exceeded",
            Self::KnowledgeLifecycleRequired => "knowledge_lifecycle_required",
            Self::UnsupportedCompletionRequirement => "unsupported_completion_requirement",
            Self::KnowledgePayloadErased => "knowledge_payload_erased",
            // Preserve the established MCP contract while returning the typed
            // artifact diagnostic in the structured error details.
            Self::InvalidPipelineArtifact(_) | Self::InvalidArgumentsDetail(_) => {
                "invalid_arguments"
            }
        }
    }

    /// Invalid tool arguments with the deserializer's reason, which names the field.
    pub fn invalid_arguments_from(reason: impl fmt::Display) -> Self {
        let mut reason = reason.to_string();
        if reason.len() > ARGUMENT_REASON_LIMIT {
            let mut end = ARGUMENT_REASON_LIMIT;
            while !reason.is_char_boundary(end) {
                end -= 1;
            }
            reason.truncate(end);
        }
        Self::InvalidArgumentsDetail(Box::new(ArgumentDiagnostic { reason }))
    }

    pub fn argument_diagnostic(&self) -> Option<&ArgumentDiagnostic> {
        match self {
            Self::InvalidArgumentsDetail(diagnostic) => Some(diagnostic),
            _ => None,
        }
    }

    pub fn pipeline_artifact_diagnostic(&self) -> Option<&PipelineArtifactDiagnostic> {
        match self {
            Self::InvalidPipelineArtifact(diagnostic) => Some(diagnostic),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
