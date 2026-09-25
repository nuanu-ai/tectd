//! Pure identities, values and invariants. No environment, transport or persistence.
mod advisory;
mod durable_knowledge;
mod durable_knowledge_validation;
mod engineering_choice_set;
mod engineering_matrix_composer;
mod engineering_matrix_disposition;
mod engineering_matrix_input;
mod engineering_matrix_native_ranking;
mod engineering_matrix_verification;
mod engineering_review;
mod error;
mod identity;
mod knowledge_change_validation;
mod knowledge_consumer;
mod knowledge_document;
mod knowledge_erased_no_change;
mod knowledge_lifecycle;
mod knowledge_lifecycle_execution;
#[cfg(test)]
mod knowledge_lifecycle_tests;
mod knowledge_lifecycle_validation;
mod knowledge_maintenance;
mod knowledge_operation_validation;
mod knowledge_phase_validation;
mod knowledge_profile_registry;
mod knowledge_search;
mod knowledge_time;
mod matrix_planning_effect;
mod matrix_planning_selection;
mod model_routing;
mod planning_knowledge;
mod program;
mod program_page;
mod state;

pub use program::{
    NewProgramInput, Program, ProgramStatus, ProgramStep, RefreshProgramKnowledge, SaveProgram,
    TextPatch, validate_program_input,
};
pub use program_page::{ProgramCursor, ProgramInput, ProgramList, ProgramPage, ProgramSummary};

pub use advisory::*;
pub use durable_knowledge::*;
pub use engineering_choice_set::*;
pub use engineering_matrix_composer::*;
pub use engineering_matrix_disposition::*;
pub use engineering_matrix_input::*;
pub use engineering_matrix_native_ranking::*;
pub use engineering_matrix_verification::*;
pub use error::{
    Error, MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VIOLATIONS, PipelineArtifactDiagnostic,
    PipelineArtifactViolation, Result,
};
pub use identity::{
    HostAuth, HostIdentity, PrincipalRole, RequestContext, validate_native_id,
    validate_workspace_key,
};
pub use knowledge_consumer::*;
pub use knowledge_document::*;
pub use knowledge_erased_no_change::*;
pub use knowledge_lifecycle::*;
pub use knowledge_lifecycle_execution::*;
pub use knowledge_maintenance::*;
pub use knowledge_profile_registry::*;
pub use knowledge_search::*;
pub use matrix_planning_effect::*;
pub use matrix_planning_selection::*;
pub use model_routing::*;
pub use planning_knowledge::*;
pub use state::{
    Created, EventKind, NativeKnowledgeChangeSummary, NativePipelineRunSummary,
    NativePlanningSummary, NativeSliceSummary, NativeWorkCandidateSummary, Session, StateStatus,
    Workspace, WorkspaceState, WorktreeSummary,
};

mod sources;
pub use sources::{
    MAX_SOURCE_PATH_BYTES, MAX_WORKTREES, RegisteredSource, SourceLocation, SourcePage,
    validate_selection,
};

mod setup;
mod setup_file;
mod setup_page;
pub use setup::{NewSetupInput, SaveSetup, Setup, SetupStatus, SetupStep, validate_setup_input};
pub use setup_file::{
    FileObservation, FilePublication, PublicationOutcome, SetupDirectory, SetupFileStatus,
    setup_path_is_granted, validate_setup_path,
};
pub use setup_page::{
    AppliedSetup, SetupContext, SetupDiscovery, SetupInput, SetupPage, SetupSummary,
};

mod candidate_delta;
mod native_planning;
mod native_planning_receipt;
mod native_planning_validation;
mod pipeline_artifacts;
mod pipeline_checkpoint;
mod pipeline_constraints;
mod pipeline_evidence;
mod pipeline_execution;
mod pipeline_execution_validation;
mod pipeline_followups;
mod pipeline_inquiry;
mod pipeline_migration;
mod pipelines;
mod refusal;
mod scope_candidate_draft;
mod scope_candidate_validation;
mod scope_candidates;
pub use candidate_delta::{
    BlockerDeltaValue, CandidateDeltaBatch, CandidateDeltaOperation, CandidateDeltaReceipt,
    CandidateDeltaTargetKind, CandidateDeltaValue, EvidenceDeltaValue, GoalDeltaValue,
    MAX_DELTA_OPERATIONS, MAX_IDEMPOTENCY_KEY_BYTES,
    stale_reasons as candidate_delta_stale_reasons,
    validate_replay as validate_candidate_delta_replay,
};
pub use native_planning::*;
pub use native_planning_receipt::NativePlanningReceiptRequest;
pub use native_planning_validation::validate_slice_graph;
pub use pipeline_checkpoint::*;
pub use pipeline_evidence::*;
pub use pipeline_execution::*;
pub use pipeline_followups::*;
pub use pipeline_inquiry::*;
pub use pipeline_migration::*;
pub use pipelines::*;
pub use refusal::{Refusal, RefusalCode};
pub use scope_candidate_draft::{
    BlockerDraft, BlockerEntity, CandidateAdded, CandidateChanged, CandidateDecision,
    CandidateDecisionKind, CandidateDelta, CandidateDraft, CandidateEntity, CandidateFinding,
    CandidateFindingSeverity, CandidateGrounding, CandidateRef, CandidateReviewDraft,
    CandidateSuperseded, CandidateSupersessionDraft, CandidateUnchanged, CoverageGoalDraft,
    CoverageGoalEntity, CoverageResolutionDraft, CoverageResolutionEntity, CoverageResolutionKind,
    DraftIdentity, EmptyCandidateDisposition, EmptyCandidateDispositionKind, EvidenceDraft,
    EvidenceEntity, EvidenceKind, ExploratoryProvenance, ProtectedChangeDisposition,
    ProtectedChangeDraft, ProtectedChangeEntity, ProtectedChangeReview, ResolvedCandidateDraft,
    ReviewCandidateSet, ReviewVerdict, SaveCandidateDraft, ScopeCandidateDraft,
    ScopeCandidateReview, SelectedScopeAdvisory,
};
pub use scope_candidates::{
    BeginCandidateSet, BeginCandidateSetOutcome, CandidateBoundary, CandidateContext,
    CandidateContextPage, CandidateContextQuery, CandidateContextView, CandidateHistoryEntry,
    CandidateHistoryStatus, CandidateInput, CandidateInputSummary, CandidateMethodSnapshot,
    CandidateProgramSummary, CandidateReceiptRequest, CandidateRuleSnapshot, CandidateSet,
    CandidateSetStatus, CandidateSetSummary, CandidateSnapshot, CandidateSnapshotMaterial,
    CandidateSourceKind, CandidateSourceRef, CandidateTextFragment, HistoricalCandidateDraft,
    MatrixDecompositionParent, ProtectedObjectRef, RecordCandidateInput, RefreshCandidateSet,
    ScopeCandidatePageItem, StoredCandidateContext, StoredHistoricalCandidateDraft,
};
