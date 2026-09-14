use crate::{
    CandidateSetSummary, KnowledgeChangePhaseId, PipelineRunStatus, ProgramSummary, SetupContext,
    SliceCandidateSetStatus, SliceState,
};
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
    pub native_planning: Vec<NativePlanningSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeWorkCandidateSummary {
    pub candidate_id: Uuid,
    pub candidate_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSliceSummary {
    pub slice_id: Uuid,
    pub slice_revision: i64,
    pub state: SliceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePipelineRunSummary {
    pub run_id: Uuid,
    pub slice_id: Uuid,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeKnowledgeChangeSummary {
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub slice_id: Uuid,
    pub status: PipelineRunStatus,
    pub current_phase_id: Option<KnowledgeChangePhaseId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativePlanningSummary {
    pub scope_id: Uuid,
    pub scope_revision: i64,
    pub candidate_set_id: Uuid,
    pub candidate_set_revision: i64,
    pub candidate_set_status: SliceCandidateSetStatus,
    pub snapshot_id: Uuid,
    pub stale: bool,
    pub eligible_work: Vec<NativeWorkCandidateSummary>,
    pub slices_needing_result: Vec<NativeSliceSummary>,
    pub pipeline_runs: Vec<NativePipelineRunSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_changes: Vec<NativeKnowledgeChangeSummary>,
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
            native_planning: Vec::new(),
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
            native_planning: Vec::new(),
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
