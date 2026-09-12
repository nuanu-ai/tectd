use crate::{FileObservation, FilePublication, Setup, SetupStatus, SetupStep, WorkspaceState};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupInput {
    pub id: Uuid,
    pub sequence: i64,
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupSummary {
    pub id: Uuid,
    pub status: SetupStatus,
    pub revision: i64,
    pub current_step: SetupStep,
}

/// DB-only saved context; it never asserts current AGENTS.md presence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupContext {
    pub task_directory: String,
    pub setup: Option<SetupSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupDiscovery {
    pub state: WorkspaceState,
    pub file: FileObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupPage {
    pub setup: Setup,
    pub inputs: Vec<SetupInput>,
    pub next_after_input: Option<i64>,
    pub file: FileObservation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedSetup {
    pub setup: Setup,
    pub publication: FilePublication,
}
