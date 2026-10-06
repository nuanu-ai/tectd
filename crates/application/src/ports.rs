use async_trait::async_trait;
use tect_domain::{
    Created, EventKind, HostAuth, HostIdentity, NewProgramInput, Program, ProgramCursor,
    ProgramInput, ProgramSummary, RegisteredSource, Result, Session, SourceLocation, Workspace,
    WorktreeSummary,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionMode {
    ReadOnly,
    ReadWrite,
}

#[async_trait]
pub trait Store: Send + Sync {
    async fn begin(&self, mode: TransactionMode) -> Result<Box<dyn UnitOfWork>>;

    async fn seal_committed_advisory_observation(
        &self,
        _continuation: &crate::AdvisoryDispatchContinuation,
        _observation: &crate::AdvisoryProviderReceiptObservation,
        _elapsed_ms: i64,
    ) -> Result<crate::StoredAdvisoryProviderReceipt> {
        Err(tect_domain::Error::Forbidden)
    }

    async fn consume_committed_advisory_observation(
        &self,
        _continuation: &crate::AdvisoryDispatchContinuation,
        _usage: crate::AdvisoryProviderReceiptUsage,
    ) -> Result<(
        crate::StoredAdvisoryProviderReceipt,
        tect_domain::AdvisoryBudgetConsumption,
    )> {
        Err(tect_domain::Error::Forbidden)
    }

    async fn seal_committed_matrix_observation(
        &self,
        _tenant_id: Uuid,
        _continuation: &crate::MatrixDispatchContinuation,
        _observation: &crate::MatrixProviderObservation,
        _elapsed_ms: i64,
    ) -> Result<crate::StoredMatrixDispatch> {
        Err(tect_domain::Error::Forbidden)
    }

    async fn consume_committed_matrix_observation(
        &self,
        _tenant_id: Uuid,
        _continuation: &crate::MatrixDispatchContinuation,
        _usage: crate::MatrixProviderUsage,
    ) -> Result<(
        crate::StoredMatrixDispatch,
        tect_domain::AdvisoryBudgetConsumption,
    )> {
        Err(tect_domain::Error::Forbidden)
    }
    async fn seal_committed_model_route_response(
        &self,
        _tenant_id: Uuid,
        _permit: &crate::ModelRouteSendPermit,
        _raw: &[u8],
    ) -> Result<()> {
        Err(tect_domain::Error::Forbidden)
    }
    async fn consume_committed_model_route_budget(
        &self,
        _tenant_id: Uuid,
        _permit: &crate::ModelRouteSendPermit,
        _observation: &crate::ModelRouteProviderObservation,
    ) -> Result<bool> {
        Err(tect_domain::Error::Forbidden)
    }
    async fn seal_committed_model_route_observation(
        &self,
        _tenant_id: Uuid,
        _permit: &crate::ModelRouteSendPermit,
        _observation: &crate::ModelRouteProviderObservation,
    ) -> Result<()> {
        Err(tect_domain::Error::Forbidden)
    }
    async fn record_committed_model_route_failure(
        &self,
        _tenant_id: Uuid,
        _permit: &crate::ModelRouteSendPermit,
    ) -> Result<()> {
        Err(tect_domain::Error::Forbidden)
    }
}

/// A dropped unit of work rolls back. No database-specific types escape this port.
#[async_trait]
pub trait UnitOfWork:
    Send
    + crate::SetupStore
    + crate::ScopeCandidateStore
    + crate::CandidateDeltaStore
    + crate::NativePlanningStore
    + crate::PipelineExecutionStore
    + crate::DurableKnowledgeStore
    + crate::KnowledgeLifecycleStore
    + crate::KnowledgeMaintenanceStore
    + crate::KnowledgeSearchStore
    + crate::PlanningKnowledgeStore
    + crate::AdvisoryStore
    + crate::ScopeAdvisoryStore
    + crate::MatrixTaskStore
    + crate::MatrixAdviceStore
    + crate::MatrixDispositionStore
    + crate::MatrixPlanningSelectionStore
    + crate::MatrixPlanningEffectStore
{
    fn model_route_selection_read(&mut self) -> Option<&mut dyn crate::ModelRouteSelectionRead> {
        None
    }
    /// The preparation path requires selection and capture on one transaction.
    fn model_route_preparation_store(
        &mut self,
    ) -> Option<&mut dyn crate::ModelRoutePreparationStore> {
        None
    }
    fn model_route_recommendation_store(
        &mut self,
    ) -> Option<&mut dyn crate::ModelRouteRecommendationStore> {
        None
    }
    fn model_route_decision_store(&mut self) -> Option<&mut dyn crate::ModelRouteDecisionStore> {
        None
    }
    /// Preparation lookup/currentness and decision capture use one transaction.
    fn model_route_decision_capture_store(
        &mut self,
    ) -> Option<&mut dyn crate::ModelRouteDecisionCaptureStore> {
        None
    }
    fn model_route_attempt_store(&mut self) -> Option<&mut dyn crate::ModelRouteAttemptStore> {
        None
    }
    fn pipeline_open_effect_store(&mut self) -> Option<&mut dyn crate::PipelineOpenEffectStore> {
        None
    }
    fn pipeline_phase_effect_store(&mut self) -> Option<&mut dyn crate::PipelinePhaseEffectStore> {
        None
    }
    /// Optional pre-open pipeline recommendation persistence.
    /// Optional source-bound anti-bloat ledger; absent adapters fail closed.
    fn anti_bloat_store(&mut self) -> Option<&mut dyn crate::AntiBloatStore> {
        None
    }
    fn anti_bloat_verification_store(
        &mut self,
    ) -> Option<&mut dyn crate::AntiBloatVerificationStore> {
        None
    }
    fn pipeline_recommendation_store(
        &mut self,
    ) -> Option<&mut dyn crate::PipelineRecommendationStore> {
        None
    }
    /// Optional one-attempt pipeline provider dispatch seam.
    fn pipeline_recommendation_dispatch_store(
        &mut self,
    ) -> Option<&mut dyn crate::PipelineRecommendationDispatchStore> {
        None
    }
    fn matrix_requirements_context_store(
        &mut self,
    ) -> Option<&mut dyn crate::MatrixRequirementsContextStore> {
        None
    }
    fn matrix_verification_store(&mut self) -> Option<&mut dyn crate::MatrixVerificationStore> {
        None
    }
    fn context_matrix_verification_store(
        &mut self,
    ) -> Option<&mut dyn crate::ContextMatrixVerificationStore> {
        None
    }
    /// An absent adapter denies budget policy lookup and installation.
    fn advisory_budget_policy_store(
        &mut self,
    ) -> Option<&mut dyn crate::AdvisoryBudgetPolicyStore> {
        None
    }
    async fn authenticate(&mut self, auth: &HostAuth) -> Result<HostIdentity>;
    async fn set_tenant(&mut self, tenant_id: Uuid) -> Result<()>;
    async fn lock_native_session(&mut self, host_id: Uuid, native_id: &str) -> Result<()>;
    async fn session(&mut self, host_id: Uuid, native_id: &str) -> Result<Option<Session>>;
    async fn session_advisory_preference(
        &mut self,
        _workspace_id: Uuid,
        _session_id: Uuid,
    ) -> Result<tect_domain::SessionAdvisoryPreference> {
        Err(tect_domain::Error::Forbidden)
    }
    async fn set_session_advisory_preference(
        &mut self,
        _workspace_id: Uuid,
        _session_id: Uuid,
        _request: &tect_domain::SetSessionAdvisoryPreference,
    ) -> Result<tect_domain::SessionAdvisoryPreference> {
        Err(tect_domain::Error::Forbidden)
    }
    async fn workspace(&mut self, id: Uuid) -> Result<Option<Workspace>>;
    async fn workspace_by_key(&mut self, key: &str) -> Result<Option<Workspace>>;
    async fn is_member(&mut self, workspace_id: Uuid, principal_id: Uuid) -> Result<bool>;
    async fn session_principal(&mut self, session_id: Uuid) -> Result<Uuid>;
    async fn ensure_workspace(&mut self, key: &str) -> Result<Created<Workspace>>;
    async fn ensure_membership(&mut self, workspace_id: Uuid, principal_id: Uuid) -> Result<()>;
    async fn ensure_session(
        &mut self,
        host_id: Uuid,
        workspace_id: Uuid,
        native_id: &str,
    ) -> Result<Created<Session>>;
    async fn append_creation_event(
        &mut self,
        workspace_id: Uuid,
        kind: EventKind,
        entity_id: Uuid,
    ) -> Result<()>;
    async fn register_source(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        location: &SourceLocation,
    ) -> Result<RegisteredSource>;
    async fn source_worktrees(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<WorktreeSummary>>;
    async fn replace_selection(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
        ids: &[Uuid],
    ) -> Result<()>;
    async fn selected_worktrees(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
    ) -> Result<Vec<WorktreeSummary>>;
    /// Return at most limit entries, in UUID order; limit may be 101 for look-ahead.
    async fn list_sources(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<RegisteredSource>>;
    async fn commit(self: Box<Self>) -> Result<()>;
    /// Create Program/input together, or return current Program for a byte-identical retry.
    async fn ensure_program(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        input: &NewProgramInput,
    ) -> Result<Program>;
    async fn program(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        for_update: bool,
    ) -> Result<Option<Program>>;
    async fn program_input(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<ProgramInput>>;
    async fn insert_program_input(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        session_id: Uuid,
        sequence: i64,
        input: &NewProgramInput,
    ) -> Result<ProgramInput>;
    async fn update_program(&mut self, program: &Program) -> Result<()>;
    async fn program_inputs(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<ProgramInput>>;
    async fn list_programs(
        &mut self,
        workspace_id: Uuid,
        after: Option<ProgramCursor>,
        limit: u32,
    ) -> Result<Vec<ProgramSummary>>;
}

/// Host encoding is measured by an adapter; inner policy owns rollback before commit.
pub trait ProgramOutputGuard: Send + Sync {
    fn input_bytes(&self, input: &str) -> Result<i64>;
    fn check(&self, program: &Program) -> Result<()>;
}

pub trait ProgramGuidance: Send + Sync {
    fn planning_method(&self) -> tect_domain::PlanningMethodSnapshot;
}

/// Host adapter validates real Git paths without mutating repositories.
#[async_trait]
pub trait SourceInspector: Send + Sync {
    async fn inspect(&self, path: &str, allowed_roots: &[String]) -> Result<SourceLocation>;
}
