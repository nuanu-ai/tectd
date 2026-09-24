use async_trait::async_trait;
use tect_domain::{
    EngineeringCandidate, Error, MatrixPlanningEffectMaterial, MatrixPlanningEffectNode, Result,
    SliceCandidateNode,
};
use uuid::Uuid;

use crate::MatrixPlanningSelectionLink;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixPlanningEffectSnapshot {
    pub link: MatrixPlanningSelectionLink,
    /// True only when the exact save_slice_draft receipt still has its result.
    pub receipt_present: bool,
    pub selected_choice: EngineeringCandidate,
    pub matrix_owner_principal_id: Uuid,
    /// Full current persisted bodies in link.mapped_nodes order.
    pub saved_nodes: Vec<SliceCandidateNode>,
    pub current_result_revision: i64,
    pub is_current: bool,
}

impl MatrixPlanningEffectSnapshot {
    pub fn material(&self, workspace_id: Uuid) -> Result<MatrixPlanningEffectMaterial> {
        let link = &self.link;
        link.selection.validate().map_err(|_| Error::StaleContext)?;
        if !self.receipt_present
            || !self.is_current
            || self.current_result_revision != link.result_revision
            || self.selected_choice.candidate_id != link.selection.selected_choice_id
            || self.saved_nodes.len() != link.mapped_nodes.len()
            || self.saved_nodes.is_empty()
            || self.matrix_owner_principal_id.is_nil()
        {
            return Err(Error::StaleContext);
        }
        let mut nodes = Vec::with_capacity(self.saved_nodes.len());
        for (mapped, body) in link.mapped_nodes.iter().zip(&self.saved_nodes) {
            if mapped.node_id != body.id() || mapped.node_revision != body.revision() {
                return Err(Error::StaleContext);
            }
            nodes.push(MatrixPlanningEffectNode {
                draft_index: mapped.draft_index,
                node_id: mapped.node_id,
                node_revision: mapped.node_revision,
                body: body.clone(),
            });
        }
        if link.selection.mapped_draft_node_indices
            != link
                .mapped_nodes
                .iter()
                .map(|n| n.draft_index)
                .collect::<Vec<_>>()
        {
            return Err(Error::StaleContext);
        }
        Ok(MatrixPlanningEffectMaterial {
            workspace_id,
            candidate_set_id: link.candidate_set_id,
            caller_request_id: link.caller_request_id,
            scope_id: link.scope_id,
            result_revision: link.result_revision,
            task_id: link.selection.task_id,
            task_revision: link.selection.task_revision,
            disposition_id: link.selection.disposition_id,
            input_digest: link.selection.expected_input_digest.clone(),
            choice_set_digest: link.selection.expected_choice_set_digest.clone(),
            verification_digest: link.selection.expected_verification_digest.clone(),
            evaluation_digest: link.evaluation_digest.clone(),
            catalogue_version: link.catalogue_version.clone(),
            caller_principal_id: link.caller_principal_id,
            caller_session_id: link.caller_session_id,
            matrix_owner_principal_id: self.matrix_owner_principal_id,
            selected_choice: self.selected_choice.clone(),
            nodes,
        })
    }

    pub fn effect_digest(&self, workspace_id: Uuid) -> Result<String> {
        self.material(workspace_id)?.canonical_digest()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatrixPlanningEffectVerdict {
    Matches,
    Rejects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixPlanningEffectAttestation {
    pub request_id: Uuid,
    pub workspace_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_request_id: Uuid,
    pub expected_result_revision: i64,
    pub effect_digest: String,
    pub verifier_principal_id: Uuid,
    pub verifier_session_id: Uuid,
    pub verdict: MatrixPlanningEffectVerdict,
    pub summary: String,
}

/// All reads are tenant/workspace scoped. The write adapter must lock and
/// rederive the current snapshot and digest in the same transaction as INSERT.
#[async_trait]
pub trait MatrixPlanningEffectStore: Send {
    async fn matrix_planning_effect_snapshot(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        caller_request_id: Uuid,
        for_update: bool,
    ) -> Result<Option<MatrixPlanningEffectSnapshot>>;

    async fn matrix_planning_effect_attestation_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<MatrixPlanningEffectAttestation>>;

    async fn append_matrix_planning_effect_attestation(
        &mut self,
        workspace_id: Uuid,
        attestation: &MatrixPlanningEffectAttestation,
    ) -> Result<()>;
}
