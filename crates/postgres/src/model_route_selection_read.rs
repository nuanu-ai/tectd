//! Read-only Slice 05 bridge from an approved Matrix-selected native save.
//! Native Work nodes do not carry typed route facts, so prose is never promoted
//! to role, tool, data, budget, latency, or host capability evidence.
use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    MatrixPlanningEffectStore, MatrixPlanningSelectionStore, ModelRouteSelectionRead,
};
use tect_domain::{
    Error, MatrixDispositionDecision, ModelRouteFact, ModelRouteSelectionLink,
    ModelRouteWorkContext, Result, SliceCandidateNode,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

fn exact_work_context(
    snapshot: tect_application::MatrixPlanningEffectSnapshot,
    disposition_id: Uuid,
    mapped_work_node_id: Uuid,
    mapped_work_node_revision: i64,
    workspace_id: Uuid,
) -> Result<ModelRouteWorkContext> {
    let material = snapshot.material(workspace_id)?;
    if material.disposition_id != disposition_id {
        return Err(Error::StaleContext);
    }
    let matching = material
        .nodes
        .iter()
        .filter(|node| {
            node.node_id == mapped_work_node_id
                && node.node_revision == mapped_work_node_revision
                && matches!(node.body, SliceCandidateNode::Work { .. })
        })
        .collect::<Vec<_>>();
    if matching.len() != 1 {
        return Err(Error::StaleContext);
    }
    let work = ModelRouteWorkContext {
        approved_matrix_selection: snapshot.link.selection,
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: material.candidate_set_id,
            caller_request_id: material.caller_request_id,
            mapped_draft_node_index: matching[0].draft_index,
            mapped_work_node_id,
            mapped_work_node_revision,
        },
        role: ModelRouteFact::Unknown,
        tool: ModelRouteFact::Unknown,
        data_class: ModelRouteFact::Unknown,
        host_capabilities: ModelRouteFact::Unknown,
        remaining_budget_units: ModelRouteFact::Unknown,
        available_latency_ms: ModelRouteFact::Unknown,
    };
    work.digest().map_err(|_| Error::StaleContext)?;
    Ok(work)
}

#[async_trait]
impl ModelRouteSelectionRead for PgUnitOfWork {
    async fn approved_work_context(
        &mut self,
        workspace_id: Uuid,
        disposition_id: Uuid,
        candidate_set_id: Uuid,
        caller_request_id: Uuid,
        mapped_work_node_id: Uuid,
        mapped_work_node_revision: i64,
    ) -> Result<Option<ModelRouteWorkContext>> {
        if workspace_id.is_nil()
            || disposition_id.is_nil()
            || candidate_set_id.is_nil()
            || caller_request_id.is_nil()
            || mapped_work_node_id.is_nil()
            || mapped_work_node_revision < 1
        {
            return Err(Error::InvalidArguments);
        }
        let Some(snapshot) = self
            .matrix_planning_effect_snapshot(
                workspace_id,
                candidate_set_id,
                caller_request_id,
                false,
            )
            .await?
        else {
            return Ok(None);
        };
        let link = &snapshot.link;
        if link.selection.disposition_id != disposition_id {
            return Err(Error::StaleContext);
        }
        let disposition = self
            .matrix_disposition_by_id(workspace_id, disposition_id)
            .await?
            .ok_or(Error::StaleContext)?;
        if disposition.request.task_id != link.selection.task_id
            || disposition.request.expected_task_revision != link.selection.task_revision
            || disposition.request.expected_input_digest != link.selection.expected_input_digest
            || disposition.request.expected_choice_set_digest.as_deref()
                != Some(link.selection.expected_choice_set_digest.as_str())
            || !matches!(
                disposition.request.decision,
                MatrixDispositionDecision::Selected { ref selected_choice_id }
                    if selected_choice_id == &link.selection.selected_choice_id
            )
        {
            return Err(Error::StaleContext);
        }
        let tenant_id = self.tenant_id()?;
        let receipt = sqlx::query(
            "SELECT request_payload,payload_erased FROM native_planning_receipts \
             WHERE tenant_id=$1 AND workspace_id=$2 AND entity_id=$3 \
               AND operation='save_slice_draft' AND request_id=$4",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(caller_request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?
        .ok_or(Error::StaleContext)?;
        let erased: bool = receipt.try_get("payload_erased").map_err(storage_error)?;
        let request: Option<Value> = receipt.try_get("request_payload").map_err(storage_error)?;
        if erased {
            return Err(Error::KnowledgePayloadErased);
        }
        let request = request.ok_or(Error::StaleContext)?;
        let expected_selection = serde_json::to_value(&link.selection).map_err(storage_error)?;
        if request.get("matrix_selection") != Some(&expected_selection)
            || request.get("request_id") != Some(&serde_json::json!(caller_request_id))
            || request.get("candidate_set_id") != Some(&serde_json::json!(candidate_set_id))
            || request.get("scope_id") != Some(&serde_json::json!(link.scope_id))
        {
            return Err(Error::StaleContext);
        }
        exact_work_context(
            snapshot,
            disposition_id,
            mapped_work_node_id,
            mapped_work_node_revision,
            workspace_id,
        )
        .map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_application::{
        MatrixPlanningEffectSnapshot, MatrixPlanningMappedNode, MatrixPlanningSelectionLink,
    };
    use tect_domain::{EngineeringCandidate, MatrixPlanningSelection, PipelineKind};

    fn snapshot() -> (Uuid, MatrixPlanningEffectSnapshot, Uuid) {
        let workspace = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let selected_choice = EngineeringCandidate {
            candidate_id: "choice-a".into(),
            title: "Choice".into(),
            approach: "Approach".into(),
            assumption_fact_ids: vec![],
        };
        let link = MatrixPlanningSelectionLink {
            selection: MatrixPlanningSelection {
                task_id: Uuid::new_v4(),
                task_revision: 1,
                disposition_id: Uuid::new_v4(),
                selected_choice_id: selected_choice.candidate_id.clone(),
                expected_input_digest: "a".repeat(64),
                expected_choice_set_digest: "b".repeat(64),
                expected_verification_digest: "c".repeat(64),
                mapped_draft_node_indices: vec![0],
            },
            evaluation_digest: "d".repeat(64),
            catalogue_version: "EM@1".into(),
            caller_principal_id: Uuid::new_v4(),
            caller_session_id: Uuid::new_v4(),
            scope_id: Uuid::new_v4(),
            candidate_set_id: Uuid::new_v4(),
            caller_request_id: Uuid::new_v4(),
            result_revision: 2,
            mapped_nodes: vec![MatrixPlanningMappedNode {
                draft_index: 0,
                node_id,
                node_revision: 1,
            }],
        };
        let snapshot = MatrixPlanningEffectSnapshot {
            link,
            receipt_present: true,
            selected_choice,
            matrix_owner_principal_id: Uuid::new_v4(),
            saved_nodes: vec![SliceCandidateNode::Work {
                id: node_id,
                revision: 1,
                title: "Implement".into(),
                outcome: "Ship".into(),
                includes: vec!["role=agent;tool=code;budget=999".into()],
                excludes: vec![],
                dependencies: vec![],
                proof: vec!["proof".into()],
                pipeline: PipelineKind::LightweightTddDevelopment,
                pipeline_reason: "Small work".into(),
                why_lightweight_insufficient: None,
                why_further_vertical_split_not_viable: None,
                source_result_ids: vec![],
                source_checkpoint: None,
            }],
            current_result_revision: 2,
            is_current: true,
        };
        (workspace, snapshot, node_id)
    }

    #[test]
    fn exact_saved_work_mapping_keeps_prose_facts_unknown() {
        let (workspace, snapshot, node_id) = snapshot();
        let disposition_id = snapshot.link.selection.disposition_id;
        let context =
            exact_work_context(snapshot.clone(), disposition_id, node_id, 1, workspace).unwrap();
        assert_eq!(
            context.selection_link.candidate_set_id,
            snapshot.link.candidate_set_id
        );
        assert_eq!(
            context.selection_link.caller_request_id,
            snapshot.link.caller_request_id
        );
        assert_eq!(context.selection_link.mapped_work_node_id, node_id);
        assert!(context.has_unknown_facts());
        assert!(matches!(context.role, ModelRouteFact::Unknown));
        assert!(matches!(context.tool, ModelRouteFact::Unknown));
        assert!(matches!(context.data_class, ModelRouteFact::Unknown));
        assert!(matches!(context.host_capabilities, ModelRouteFact::Unknown));
        assert!(matches!(
            context.remaining_budget_units,
            ModelRouteFact::Unknown
        ));
        assert!(matches!(
            context.available_latency_ms,
            ModelRouteFact::Unknown
        ));
    }

    #[test]
    fn stale_wrong_or_decision_mapping_fails_closed() {
        let (workspace, snapshot, node_id) = snapshot();
        let disposition_id = snapshot.link.selection.disposition_id;
        assert!(
            exact_work_context(
                snapshot.clone(),
                disposition_id,
                Uuid::new_v4(),
                1,
                workspace
            )
            .is_err()
        );
        assert!(
            exact_work_context(snapshot.clone(), disposition_id, node_id, 2, workspace).is_err()
        );
        assert!(
            exact_work_context(snapshot.clone(), Uuid::new_v4(), node_id, 1, workspace).is_err()
        );
        let mut stale = snapshot.clone();
        stale.is_current = false;
        assert!(exact_work_context(stale, disposition_id, node_id, 1, workspace).is_err());
        let mut decision = snapshot;
        decision.saved_nodes[0] = SliceCandidateNode::Decision {
            id: node_id,
            revision: 1,
            title: "Decision".into(),
            question: "Why?".into(),
            resolution_criteria: vec!["Proof".into()],
            dependencies: vec![],
            source_result_ids: vec![],
        };
        assert!(exact_work_context(decision, disposition_id, node_id, 1, workspace).is_err());
    }
}
