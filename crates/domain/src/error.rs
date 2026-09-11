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
    InputPending,
    ProgramIncomplete,
    InputConflict,
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
            Self::InputPending => "input_pending",
            Self::ProgramIncomplete => "program_incomplete",
            Self::InputConflict => "input_conflict",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
