use crate::{Refusal, RefusalCode};
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
    BudgetPolicyInvalid,
    BudgetExhaustedBeforeDispatch,
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
    Refused(Box<Refusal>),
    /// Add a strict pipeline refusal while retaining the established outer
    /// error code and recovery behavior for compatibility.
    PipelineRefused {
        source: Box<Error>,
        refusal: Box<Refusal>,
    },
    InvalidPipelineArtifact(Box<PipelineArtifactDiagnostic>),
    /// Tool arguments failed to deserialize; the reason names the offending field.
    InvalidArgumentsDetail(Box<ArgumentDiagnostic>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArgumentDiagnostic {
    pub reason: String,
    pub violation_code: String,
    pub pointer: String,
    pub expected: String,
    pub actual: String,
}

/// Upper bound for a deserializer reason carried to the agent.
const ARGUMENT_REASON_LIMIT: usize = 600;

mod pipeline_artifact_diagnostic;
pub use pipeline_artifact_diagnostic::{
    MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VIOLATIONS, PipelineArtifactDiagnostic,
    PipelineArtifactViolation,
};

impl Error {
    pub fn refused(code: RefusalCode, next_action: &'static str, required: &'static str) -> Self {
        Self::Refused(Box::new(
            Refusal::new(code)
                .with_next_action(next_action)
                .with_required(required),
        ))
    }

    pub fn refused_at(
        code: RefusalCode,
        rule: &'static str,
        path: &'static str,
        expected: impl Into<String>,
        actual: impl Into<String>,
        next_action: &'static str,
        required: &'static str,
    ) -> Self {
        Self::Refused(Box::new(
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

    pub fn refused_backend_proof(path: &'static str) -> Self {
        Self::Refused(Box::new(
            Refusal::new(RefusalCode::BackendDerivedProofRequired)
                .with_message(RefusalCode::BackendDerivedProofRequired.message())
                .with_next_action("omit_agent_supplied_proof")
                .with_required("backend_derived_proof")
                .with_rule("WP3-PROOF-01")
                .with_path(path)
                .with_expected("omitted; backend derives the proof")
                .with_actual("agent-supplied value"),
        ))
    }

    /// Attach the strict pipeline refusal envelope at the common pipeline
    /// boundary. Infrastructure/session errors intentionally remain generic.
    pub fn normalize_pipeline_refusal(
        self,
        rule: &'static str,
        path: &'static str,
        expected: &'static str,
        next_action: &'static str,
        required: &'static str,
    ) -> Self {
        if matches!(
            self,
            Self::StorageUnavailable
                | Self::TransportUnavailable
                | Self::Unauthorized
                | Self::InvalidNativeSession
                | Self::InvalidWorkspaceKey
                | Self::SessionWorkspaceMismatch
                | Self::SessionRevoked
                | Self::WorkspaceNotOpen
                | Self::InvalidConfiguration
        ) {
            return self;
        }
        if matches!(self, Self::PipelineRefused { .. }) {
            return self;
        }
        let actual = self.code().to_owned();
        let refusal = self.refusal().unwrap_or_else(|| match &self {
            Self::NotFound | Self::KnowledgePayloadErased => {
                Refusal::new(RefusalCode::EvidenceMissing)
            }
            Self::Forbidden => Refusal::new(RefusalCode::AuthorityRequired),
            _ => Refusal::new(RefusalCode::UnknownCause),
        });
        let refusal =
            refusal.normalize_pipeline(rule, path, expected, actual, next_action, required);
        match self {
            Self::Refused(_) => Self::Refused(Box::new(refusal)),
            source => Self::PipelineRefused {
                source: Box::new(source),
                refusal: Box::new(refusal),
            },
        }
    }

    /// Project a legacy domain error into the additive typed refusal contract.
    pub fn refusal(&self) -> Option<Refusal> {
        if let Self::Refused(refusal) | Self::PipelineRefused { refusal, .. } = self {
            return Some((**refusal).clone());
        }
        let (code, next_action, required) = match self {
            Self::StaleRevision => (RefusalCode::StaleRevision, "refresh", "revision"),
            Self::InputConflict => (
                RefusalCode::IdempotencyConflict,
                "reuse_or_replace_request",
                "request_id",
            ),
            Self::InvalidPipelineArtifact(_) => {
                (RefusalCode::InvalidOutput, "correct_output", "output")
            }
            Self::RequestTooLarge | Self::CapacityExceeded => {
                (RefusalCode::PayloadTooLarge, "reduce_payload", "payload")
            }
            Self::KnowledgeUnavailable => {
                (RefusalCode::EvidenceMissing, "refresh_evidence", "evidence")
            }
            Self::KnowledgeLifecycleRequired => (
                RefusalCode::ArtifactNotReady,
                "complete_owner_lifecycle",
                "artifact",
            ),
            Self::InvalidArguments | Self::InvalidArgumentsDetail(_) => (
                RefusalCode::InputSchemaInvalid,
                "correct_input_and_retry",
                "schema_valid_input",
            ),
            Self::InternalInvariant => (RefusalCode::UnknownCause, "retry_exact_request", "cause"),
            Self::UnsupportedCompletionRequirement => (
                RefusalCode::MethodVersionUnavailable,
                "select_supported_method",
                "method_version",
            ),
            Self::NeedsContext | Self::InputPending => (
                RefusalCode::DeliveryRefreshRequired,
                "refresh_context",
                "context",
            ),
            Self::ContextChanged | Self::StaleContext => (
                RefusalCode::DependencyStale,
                "refresh_dependencies",
                "dependency",
            ),
            Self::Unauthorized | Self::Forbidden => (
                RefusalCode::AuthorityRequired,
                "obtain_authority",
                "authority",
            ),
            Self::Refused(refusal) if refusal.code == RefusalCode::LegacyMigrationRequired => (
                RefusalCode::LegacyMigrationRequired,
                "provide_explicit_successor_mapping",
                "predecessor_successor_obligation_evidence_metadata",
            ),
            _ => return None,
        };
        Some(
            Refusal::new(code)
                .with_next_action(next_action)
                .with_required(required),
        )
    }

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
            Self::BudgetPolicyInvalid => "budget_policy_invalid",
            Self::BudgetExhaustedBeforeDispatch => "budget_exhausted_before_dispatch",
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
            Self::Refused(refusal) => refusal.code.as_str(),
            Self::PipelineRefused { source, .. } => source.code(),
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
        let quoted = |prefix: &str| {
            reason
                .strip_prefix(prefix)
                .and_then(|value| value.split('`').nth(1))
                .map(str::to_owned)
        };
        let (violation_code, field, expected, actual) =
            if let Some(field) = quoted("missing field ") {
                (
                    "required_field_missing",
                    Some(field.clone()),
                    format!("field `{field}`"),
                    "missing".to_owned(),
                )
            } else if let Some(field) = quoted("unknown field ") {
                (
                    "field_forbidden",
                    Some(field.clone()),
                    "field omitted by current schema".to_owned(),
                    format!("field `{field}` supplied"),
                )
            } else if reason.starts_with("unknown variant ") {
                (
                    "enum_value_invalid",
                    None,
                    "one advertised enum value".to_owned(),
                    reason.clone(),
                )
            } else if reason.starts_with("invalid type:") {
                (
                    "type_invalid",
                    None,
                    "advertised JSON type".to_owned(),
                    reason.clone(),
                )
            } else {
                (
                    "schema_constraint_failed",
                    None,
                    "current route schema".to_owned(),
                    reason.clone(),
                )
            };
        let pointer = field
            .map(|field| format!("/params/{}", field.replace('~', "~0").replace('/', "~1")))
            .unwrap_or_else(|| "/params".to_owned());
        Self::InvalidArgumentsDetail(Box::new(ArgumentDiagnostic {
            reason,
            violation_code: violation_code.to_owned(),
            pointer,
            expected,
            actual,
        }))
    }

    pub fn invalid_arguments_at(reason: impl fmt::Display, pointer: impl Into<String>) -> Self {
        let mut error = Self::invalid_arguments_from(reason);
        if let Self::InvalidArgumentsDetail(diagnostic) = &mut error {
            diagnostic.pointer = pointer.into();
        }
        error
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

    pub fn pipeline_source(&self) -> &Self {
        match self {
            Self::PipelineRefused { source, .. } => source.pipeline_source(),
            _ => self,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}

#[cfg(test)]
#[path = "error/tests.rs"]
mod tests;
