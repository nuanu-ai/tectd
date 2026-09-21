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
mod tests {
    use super::*;

    #[test]
    fn legacy_errors_map_to_typed_refusals() {
        assert_eq!(
            Error::StaleRevision.refusal().unwrap().code,
            RefusalCode::StaleRevision
        );
        assert_eq!(
            Error::InputConflict.refusal().unwrap().code,
            RefusalCode::IdempotencyConflict
        );
        assert_eq!(
            Error::RequestTooLarge.refusal().unwrap().code,
            RefusalCode::PayloadTooLarge
        );
        assert!(Error::NotFound.refusal().is_none());
    }

    #[test]
    fn exhaustive_pipeline_branch_table_has_complete_stable_metadata() {
        struct Branch {
            name: &'static str,
            code: RefusalCode,
            rule: &'static str,
            path: &'static str,
            expected: &'static str,
            actual: &'static str,
            next_action: &'static str,
            required: &'static str,
        }
        let branches = [
            Branch {
                name: "stale_revision",
                code: RefusalCode::StaleRevision,
                rule: "WP6-REVISION-01",
                path: "arguments.params.run_revision",
                expected: "current run revision",
                actual: "stale_revision",
                next_action: "refresh_pipeline_context",
                required: "current_revision",
            },
            Branch {
                name: "stale_checkpoint_input",
                code: RefusalCode::StaleRevision,
                rule: "WP6-CHECKPOINT-REVISION-01",
                path: "arguments.params.checkpoint_revision",
                expected: "current checkpoint revision",
                actual: "stale_revision",
                next_action: "refresh_checkpoint",
                required: "current_checkpoint_revision",
            },
            Branch {
                name: "idempotency_conflict",
                code: RefusalCode::IdempotencyConflict,
                rule: "WP6-IDEMPOTENCY-01",
                path: "arguments.params.request_id",
                expected: "unused id or exact replay payload",
                actual: "idempotency_conflict",
                next_action: "reuse_exact_payload_or_new_request_id",
                required: "consistent_request_identity",
            },
            Branch {
                name: "invalid_output",
                code: RefusalCode::InvalidOutput,
                rule: "WP6-OUTPUT-01",
                path: "arguments.params.output",
                expected: "phase output contract",
                actual: "invalid_output",
                next_action: "correct_phase_output",
                required: "valid_output",
            },
            Branch {
                name: "payload_too_large",
                code: RefusalCode::PayloadTooLarge,
                rule: "WP6-SIZE-01",
                path: "arguments.params.output",
                expected: "payload within advertised byte limit",
                actual: "payload_too_large",
                next_action: "reduce_output",
                required: "bounded_output",
            },
            Branch {
                name: "evidence_missing",
                code: RefusalCode::EvidenceMissing,
                rule: "WP6-EVIDENCE-01",
                path: "arguments.params.output.evidence_artifacts",
                expected: "all required evidence references",
                actual: "evidence_missing",
                next_action: "register_required_evidence",
                required: "complete_evidence",
            },
            Branch {
                name: "evidence_scope_mismatch",
                code: RefusalCode::EvidenceMissing,
                rule: "WP6-EVIDENCE-SCOPE-01",
                path: "arguments.params.output.evidence_artifacts",
                expected: "evidence owned by this run and phase scope",
                actual: "evidence_scope_mismatch",
                next_action: "use_in_scope_evidence",
                required: "in_scope_evidence",
            },
            Branch {
                name: "artifact_not_ready",
                code: RefusalCode::ArtifactNotReady,
                rule: "WP6-ARTIFACT-01",
                path: "arguments.params.output.evidence_artifacts",
                expected: "ready artifact revision",
                actual: "artifact_not_ready",
                next_action: "finalize_evidence_artifact",
                required: "ready_artifact",
            },
            Branch {
                name: "ambiguous_requirement",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-REQUIREMENT-01",
                path: "arguments.params.output.fields",
                expected: "one unambiguous requirement disposition",
                actual: "ambiguous_requirement",
                next_action: "clarify_requirement",
                required: "unambiguous_requirement",
            },
            Branch {
                name: "unknown_cause",
                code: RefusalCode::UnknownCause,
                rule: "WP6-CAUSE-01",
                path: "arguments.params.output.fields.cause",
                expected: "evidence-backed cause or explicit unresolved state",
                actual: "unknown_cause",
                next_action: "investigate_cause",
                required: "known_or_explicitly_unresolved_cause",
            },
            Branch {
                name: "no_test_target",
                code: RefusalCode::NoTestTarget,
                rule: "WP6-TEST-01",
                path: "arguments.params.output.fields.selected_test_target",
                expected: "executable test target",
                actual: "no_test_target",
                next_action: "select_test_target",
                required: "selected_test_target",
            },
            Branch {
                name: "review_required",
                code: RefusalCode::ReviewRequired,
                rule: "WP6-REVIEW-01",
                path: "arguments.params.output.reviewer_context",
                expected: "required fresh review decision",
                actual: "review_required",
                next_action: "complete_required_review",
                required: "ready_review",
            },
            Branch {
                name: "authority_required",
                code: RefusalCode::AuthorityRequired,
                rule: "WP6-AUTHORITY-01",
                path: "request_context.principal",
                expected: "principal authorized for protected transition",
                actual: "authority_required",
                next_action: "obtain_authority",
                required: "authorized_principal",
            },
            Branch {
                name: "environment_mismatch",
                code: RefusalCode::AuthorityRequired,
                rule: "WP6-ENVIRONMENT-01",
                path: "arguments.params.output.fields.environment",
                expected: "authorized target environment",
                actual: "environment_mismatch",
                next_action: "select_authorized_environment",
                required: "authorized_environment",
            },
            Branch {
                name: "method_version_unavailable",
                code: RefusalCode::MethodVersionUnavailable,
                rule: "WP6-VERSION-01",
                path: "arguments.params.definition_version",
                expected: "available immutable definition version",
                actual: "method_version_unavailable",
                next_action: "select_supported_method",
                required: "supported_method_version",
            },
            Branch {
                name: "delivery_refresh_required",
                code: RefusalCode::DeliveryRefreshRequired,
                rule: "WP6-DELIVERY-REFRESH-01",
                path: "arguments.params.run_id",
                expected: "fresh delivered context",
                actual: "delivery_refresh_required",
                next_action: "refresh_pipeline_context",
                required: "fresh_delivery",
            },
            Branch {
                name: "legacy_migration_required",
                code: RefusalCode::LegacyMigrationRequired,
                rule: "WP6-MIGRATION-01",
                path: "arguments.params",
                expected: "explicit successor and obligation evidence mapping",
                actual: "legacy_migration_required",
                next_action: "provide_explicit_successor_mapping",
                required: "successor_mapping",
            },
            Branch {
                name: "coverage_incomplete",
                code: RefusalCode::CoverageIncomplete,
                rule: "WP6-COVERAGE-01",
                path: "arguments.params.output.fields.coverage",
                expected: "coverage or explicit blocker for every finite obligation",
                actual: "coverage_incomplete",
                next_action: "complete_coverage",
                required: "complete_coverage",
            },
            Branch {
                name: "dependency_stale",
                code: RefusalCode::DependencyStale,
                rule: "WP6-DEPENDENCY-01",
                path: "arguments.params.consumed_knowledge",
                expected: "current backend-bound dependency",
                actual: "dependency_stale",
                next_action: "refresh_dependencies",
                required: "current_dependency",
            },
            Branch {
                name: "effect_status_unknown",
                code: RefusalCode::EffectStatusUnknown,
                rule: "WP6-EFFECT-01",
                path: "arguments.params.output.fields.effect_status",
                expected: "verified effect status",
                actual: "effect_status_unknown",
                next_action: "reconcile_effect_status",
                required: "known_effect_status",
            },
            Branch {
                name: "backend_proof",
                code: RefusalCode::BackendDerivedProofRequired,
                rule: "WP6-PROOF-01",
                path: "arguments.params.consumed_outputs",
                expected: "omitted backend-derived proof",
                actual: "backend_derived_proof_required",
                next_action: "omit_agent_supplied_proof",
                required: "backend_derived_proof",
            },
            Branch {
                name: "schema_context",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-CONTEXT-01",
                path: "arguments.params",
                expected: "context schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_instruction",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-INSTRUCTION-01",
                path: "arguments.params",
                expected: "instruction schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_begin",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-BEGIN-01",
                path: "arguments.params",
                expected: "begin schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_migration",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-MIGRATION-01",
                path: "arguments.params",
                expected: "migration schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_complete",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-COMPLETE-01",
                path: "arguments.params",
                expected: "completion schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_input",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-INPUT-01",
                path: "arguments.params",
                expected: "input schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_delivery",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-DELIVERY-01",
                path: "arguments.params",
                expected: "delivery schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_checkpoint",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-CHECKPOINT-01",
                path: "arguments.params",
                expected: "checkpoint schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_evidence_write",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-EVIDENCE-WRITE-01",
                path: "arguments.params",
                expected: "evidence write schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
            Branch {
                name: "schema_evidence_read",
                code: RefusalCode::AmbiguousRequirement,
                rule: "WP6-SCHEMA-EVIDENCE-READ-01",
                path: "arguments.params",
                expected: "evidence read schema",
                actual: "invalid_arguments",
                next_action: "read_schema_and_retry",
                required: "valid_pipeline_arguments",
            },
        ];
        assert_eq!(branches.len(), 31);
        for branch in branches {
            let refusal = Refusal::new(branch.code).normalize_pipeline(
                branch.rule,
                branch.path,
                branch.expected,
                branch.actual,
                branch.next_action,
                branch.required,
            );
            assert!(refusal.is_complete_pipeline_refusal(), "{}", branch.name);
            assert_eq!(refusal.code, branch.code, "{}", branch.name);
            assert_eq!(
                refusal.rule.as_deref(),
                Some(branch.rule),
                "{}",
                branch.name
            );
            assert_eq!(
                refusal.path.as_deref(),
                Some(branch.path),
                "{}",
                branch.name
            );
            assert_eq!(
                refusal.expected.as_deref(),
                Some(branch.expected),
                "{}",
                branch.name
            );
            assert_eq!(
                refusal.actual.as_deref(),
                Some(branch.actual),
                "{}",
                branch.name
            );
            assert_eq!(
                refusal.next_action.as_deref(),
                Some(branch.next_action),
                "{}",
                branch.name
            );
            assert_eq!(
                refusal.required.as_deref(),
                Some(branch.required),
                "{}",
                branch.name
            );
        }
    }
}
