use crate::{CandidateSetSummary, ProgramSummary, SetupContext};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub host_id: Uuid,
    pub native_session_id: String,
    pub revoked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSummary {
    pub id: Uuid,
    pub repository_id: Uuid,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateStatus {
    Uninitialized,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub status: StateStatus,
    pub workspace: Option<Workspace>,
    pub session: Option<Session>,
    pub selected_worktrees: Vec<WorktreeSummary>,
    pub next_action: Option<String>,
    pub programs: Vec<ProgramSummary>,
    pub next_after: Option<String>,
    pub setup_context: Option<SetupContext>,
    pub candidate_sets: Vec<CandidateSetSummary>,
}

impl WorkspaceState {
    pub fn unopened() -> Self {
        Self {
            status: StateStatus::Uninitialized,
            workspace: None,
            session: None,
            selected_worktrees: Vec::new(),
            next_action: Some("open_workspace".into()),
            programs: Vec::new(),
            next_after: None,
            setup_context: None,
            candidate_sets: Vec::new(),
        }
    }

    pub fn opened(workspace: Workspace, session: Session) -> Self {
        Self {
            status: StateStatus::Ready,
            workspace: Some(workspace),
            session: Some(session),
            selected_worktrees: Vec::new(),
            next_action: Some("inspect_setup".into()),
            programs: Vec::new(),
            next_after: None,
            setup_context: None,
            candidate_sets: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct Created<T> {
    pub value: T,
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    WorkspaceOpened,
    SessionOpened,
}

impl EventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceOpened => "workspace_opened",
            Self::SessionOpened => "session_opened",
        }
    }
}
