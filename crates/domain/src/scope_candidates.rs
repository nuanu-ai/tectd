use crate::{
    BlockerEntity, CandidateEntity, CoverageGoalEntity, EvidenceEntity, Program,
    ReviewCandidateSet, SaveCandidateDraft, ScopeCandidateReview, WorktreeSummary,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateBoundary {
    Finite,
    Ongoing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSetStatus {
    Draft,
    ReviewRequired,
    Ready,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSet {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub program_id: Uuid,
    pub revision: i64,
    pub status: CandidateSetStatus,
    pub boundary: CandidateBoundary,
    pub current_snapshot_id: Uuid,
    pub input_cursor: i64,
    pub latest_input: i64,
    #[serde(skip)]
    pub max_input_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSetSummary {
    pub id: Uuid,
    pub program_id: Uuid,
    pub revision: i64,
    pub status: CandidateSetStatus,
    pub boundary: CandidateBoundary,
    pub snapshot_id: Uuid,
    pub input_cursor: i64,
    pub latest_input: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateInput {
    pub id: Uuid,
    pub sequence: i64,
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateInputSummary {
    pub id: Uuid,
    pub sequence: i64,
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub source_ref_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateProgramSummary {
    pub id: Uuid,
    pub status: crate::ProgramStatus,
    pub revision: i64,
    pub current_step: crate::ProgramStep,
    pub input_cursor: i64,
    pub latest_input: i64,
    pub field_refs: Vec<CandidateSourceRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateSourceKind {
    ProgramField,
    ProgramSuccess,
    PlanningInput,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSourceRef {
    pub id: Uuid,
    pub kind: CandidateSourceKind,
    pub input_sequence: Option<i64>,
    pub program_field: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateMethodSnapshot {
    pub id: String,
    pub revision: String,
    pub digest: String,
    pub body: String,
    pub origin_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateRuleSnapshot {
    pub id: String,
    pub revision: String,
    pub text: String,
    pub origin_refs: Vec<String>,
    pub applicability: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateSnapshot {
    pub id: Uuid,
    pub sequence: i64,
    pub program_revision: i64,
    pub program_latest_input: i64,
    pub planning_latest_input: i64,
    pub selected_worktree_ids: Vec<Uuid>,
    pub selected_sources_digest: String,
    pub method: CandidateMethodSnapshot,
    pub registry_revision: String,
    pub registry_digest: String,
    pub rules: Vec<CandidateRuleSnapshot>,
    pub source_refs: Vec<CandidateSourceRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateSnapshotMaterial {
    pub program: Program,
    pub selected_worktrees: Vec<WorktreeSummary>,
    pub selected_sources_digest: String,
    pub method: CandidateMethodSnapshot,
    pub registry_revision: String,
    pub registry_digest: String,
    pub rules: Vec<CandidateRuleSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateContextView {
    Overview,
    Program,
    Inputs,
    Candidates,
    Reviews,
    History,
    Historical,
    Fragment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateContextQuery {
    pub candidate_set_id: Uuid,
    pub view: CandidateContextView,
    pub draft_revision: Option<i64>,
    pub after: Option<i64>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginCandidateSet {
    pub request_id: Uuid,
    pub program_id: Uuid,
    pub program_revision: i64,
    pub boundary: CandidateBoundary,
    pub input: String,
    #[serde(default)]
    pub task_context: crate::PlanningTaskContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordCandidateInput {
    pub candidate_set_id: Uuid,
    pub revision: i64,
    pub request_id: Uuid,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshCandidateSet {
    pub candidate_set_id: Uuid,
    pub revision: i64,
    pub request_id: Uuid,
    pub program_revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_context: Option<crate::PlanningTaskContext>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeginCandidateSetOutcome {
    Created(CandidateContext),
    Replay(CandidateContext),
    Existing(CandidateContext),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateContext {
    pub candidate_set: CandidateSet,
    pub snapshot: CandidateSnapshot,
    pub current_program_revision: i64,
    pub stale_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_knowledge: Option<crate::PlanningKnowledgeStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeCandidatePageItem {
    Input(CandidateInputSummary),
    Candidate(CandidateEntity),
    Goal(CoverageGoalEntity),
    Evidence(EvidenceEntity),
    Blocker(BlockerEntity),
    Review(ScopeCandidateReview),
    History(CandidateHistoryEntry),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateHistoryStatus {
    Active,
    Prior,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateHistoryEntry {
    pub candidate_id: Uuid,
    pub candidate_revision: i64,
    pub title: String,
    pub first_draft_revision: i64,
    pub latest_draft_revision: i64,
    pub latest_snapshot_id: Uuid,
    pub status: CandidateHistoryStatus,
    pub superseded_reason: Option<String>,
    pub replacement_candidate_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoricalCandidateDraft {
    pub set_revision: i64,
    pub snapshot: CandidateSnapshot,
    pub input_cursor: i64,
    pub boundary: CandidateBoundary,
    pub delta: crate::CandidateDelta,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateTextFragment {
    pub source_ref: CandidateSourceRef,
    pub snapshot_id: Uuid,
    pub cursor: usize,
    pub next_cursor: Option<usize>,
    pub next_source_ref_id: Option<Uuid>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedObjectRef {
    pub accepted_evidence_id: Uuid,
    pub prior_candidate_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateContextPage {
    pub context: CandidateContext,
    pub view: CandidateContextView,
    pub program: Option<CandidateProgramSummary>,
    pub historical: Option<HistoricalCandidateDraft>,
    pub items: Vec<ScopeCandidatePageItem>,
    pub next_after: Option<i64>,
    pub required_protected_changes: Vec<ProtectedObjectRef>,
    pub terminal_note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredHistoricalCandidateDraft {
    pub snapshot: CandidateSnapshot,
    pub input_cursor: i64,
    pub draft: crate::ResolvedCandidateDraft,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredCandidateContext {
    pub context: CandidateContext,
    pub program: Program,
    pub draft: Option<crate::ResolvedCandidateDraft>,
    pub reviews: Vec<ScopeCandidateReview>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateReceiptRequest {
    SaveDraft(SaveCandidateDraft),
    Review(ReviewCandidateSet),
    RecordInput(RecordCandidateInput),
    Refresh(RefreshCandidateSet),
}

impl CandidateReceiptRequest {
    pub fn candidate_set_id(&self) -> Uuid {
        match self {
            Self::SaveDraft(value) => value.candidate_set_id,
            Self::Review(value) => value.candidate_set_id,
            Self::RecordInput(value) => value.candidate_set_id,
            Self::Refresh(value) => value.candidate_set_id,
        }
    }

    pub fn request_id(&self) -> Uuid {
        match self {
            Self::SaveDraft(value) => value.request_id,
            Self::Review(value) => value.request_id,
            Self::RecordInput(value) => value.request_id,
            Self::Refresh(value) => value.request_id,
        }
    }

    pub fn operation(&self) -> &'static str {
        match self {
            Self::SaveDraft(_) => "save_draft",
            Self::Review(_) => "save_review",
            Self::RecordInput(_) => "record_input",
            Self::Refresh(_) => "refresh",
        }
    }
}
