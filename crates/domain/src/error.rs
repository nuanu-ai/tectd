use serde::{Deserialize, Serialize};
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

/// Stable public errors deliberately contain no database or credential details.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

impl Error {
    pub const fn code(self) -> &'static str {
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
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
