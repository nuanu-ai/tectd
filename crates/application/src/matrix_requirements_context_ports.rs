use async_trait::async_trait;
use serde_json::{Value, json};
use tect_domain::{
    EffectiveMatrixRequirements, MatrixRequirementsConfirmation, MatrixRequirementsProposal,
    MatrixRequirementsRevision, RequirementDeclarationPatch, RequirementsAnchor, Result,
};
use uuid::Uuid;

/// Locators are checked against persisted ancestry, never accepted as lineage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatrixRequirementsLocator {
    Program {
        program_id: Uuid,
    },
    Scope {
        program_id: Uuid,
        scope_id: Uuid,
    },
    Slice {
        program_id: Uuid,
        scope_id: Uuid,
        candidate_set_id: Uuid,
        work_candidate_id: Uuid,
        expected_work_revision: i64,
    },
    OpenedSlice {
        slice_id: Uuid,
    },
}
impl MatrixRequirementsLocator {
    pub fn as_json(&self) -> Value {
        match *self {
            Self::Program { program_id } => json!({"level":"program","program_id":program_id}),
            Self::Scope {
                program_id,
                scope_id,
            } => json!({"level":"scope","program_id":program_id,"scope_id":scope_id}),
            Self::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            } => {
                json!({"level":"slice","program_id":program_id,"scope_id":scope_id,"candidate_set_id":candidate_set_id,"work_candidate_id":work_candidate_id,"expected_work_revision":expected_work_revision})
            }
            Self::OpenedSlice { slice_id } => json!({"level":"opened_slice","slice_id":slice_id}),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposeMatrixRequirementsContext {
    pub request_id: Uuid,
    pub locator: MatrixRequirementsLocator,
    pub expected_context_revision: u64,
    pub patches: Vec<RequirementDeclarationPatch>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmMatrixRequirementsContext {
    pub request_id: Uuid,
    pub locator: MatrixRequirementsLocator,
    pub proposal_revision: u64,
    pub proposal_digest: String,
    pub owner_response_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMatrixRequirementsProposal {
    pub request: ProposeMatrixRequirementsContext,
    pub proposal: MatrixRequirementsProposal,
    pub recorded_at_epoch_seconds: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMatrixRequirementsConfirmation {
    pub request: ConfirmMatrixRequirementsContext,
    pub confirmation: MatrixRequirementsConfirmation,
    pub recorded_at_epoch_seconds: i64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenMatrixRequirementsContext {
    pub id: Uuid,
    pub anchor: RequirementsAnchor,
    pub effective: EffectiveMatrixRequirements,
    pub payload_sha256: String,
}

/// Every method operates within the caller's existing tenant-bound transaction.
#[async_trait]
pub trait MatrixRequirementsContextStore: Send {
    /// Resolve accepted Scope/saved Work/opening origin, reject cross-parent
    /// locators, and enforce current anchor ACL for this principal. Returns
    /// exact validated Program -> Scope -> logical Slice ancestry.
    async fn matrix_requirements_lineage(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        locator: &MatrixRequirementsLocator,
        for_write: bool,
    ) -> Result<Vec<RequirementsAnchor>>;
    async fn matrix_requirements_proposal_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<StoredMatrixRequirementsProposal>>;
    async fn matrix_requirements_confirmation_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<StoredMatrixRequirementsConfirmation>>;
    /// Lock the anchor head for write, returning zero for an unused anchor.
    async fn lock_matrix_requirements_head(
        &mut self,
        workspace_id: Uuid,
        anchor: RequirementsAnchor,
    ) -> Result<u64>;
    async fn matrix_requirements_revisions(
        &mut self,
        workspace_id: Uuid,
        lineage: &[RequirementsAnchor],
    ) -> Result<Vec<MatrixRequirementsRevision>>;
    /// CAS append only: assert current head equals expected revision.
    async fn append_matrix_requirements_proposal(
        &mut self,
        workspace_id: Uuid,
        expected_revision: u64,
        record: &StoredMatrixRequirementsProposal,
    ) -> Result<()>;
    /// Append only, unique per exact proposal. Never replace a confirmation.
    async fn append_matrix_requirements_confirmation(
        &mut self,
        workspace_id: Uuid,
        record: &StoredMatrixRequirementsConfirmation,
    ) -> Result<()>;
    /// Future material consumers freeze in their existing write UOW, never GET.
    async fn append_frozen_matrix_requirements(
        &mut self,
        workspace_id: Uuid,
        snapshot: &FrozenMatrixRequirementsContext,
    ) -> Result<FrozenMatrixRequirementsContext>;

    /// Read only the exact immutable snapshot in this tenant and workspace.
    /// Adapters must verify canonical bytes and semantic integrity before use.
    async fn frozen_matrix_requirements_by_id(
        &mut self,
        _workspace_id: Uuid,
        _snapshot_id: Uuid,
    ) -> Result<Option<FrozenMatrixRequirementsContext>> {
        Err(tect_domain::Error::Forbidden)
    }
}
