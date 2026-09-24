use async_trait::async_trait;
use tect_domain::{MatrixPlanningSelection, Result};
use uuid::Uuid;

use crate::MatrixDispositionRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixPlanningMappedNode {
    pub draft_index: usize,
    pub node_id: Uuid,
    pub node_revision: i64,
}

/// Server-validated binding to one real native planning save receipt.
/// `evaluation_digest` covers current verified input, composition and choice set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixPlanningSelectionLink {
    pub selection: MatrixPlanningSelection,
    pub evaluation_digest: String,
    pub catalogue_version: String,
    pub caller_principal_id: Uuid,
    pub caller_session_id: Uuid,
    pub scope_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_request_id: Uuid,
    pub result_revision: i64,
    pub mapped_nodes: Vec<MatrixPlanningMappedNode>,
}

/// All methods run in the native save's unit of work. The adapter must check
/// the exact persisted `save_slice_draft` receipt before inserting a link and
/// return the original link only for a byte-identical replay.
#[async_trait]
pub trait MatrixPlanningSelectionStore: Send {
    async fn matrix_planning_selection_link(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        caller_request_id: Uuid,
    ) -> Result<Option<MatrixPlanningSelectionLink>>;

    async fn matrix_disposition_by_id(
        &mut self,
        workspace_id: Uuid,
        disposition_id: Uuid,
    ) -> Result<Option<MatrixDispositionRecord>>;

    async fn link_matrix_planning_selection(
        &mut self,
        workspace_id: Uuid,
        link: &MatrixPlanningSelectionLink,
    ) -> Result<()>;
}
