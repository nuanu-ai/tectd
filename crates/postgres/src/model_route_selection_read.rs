//! Read-only Slice 05 bridge from an approved Matrix-selected native save.
//! The exact saved Work and V2 Matrix context are rechecked before advice.
//! Caller-authored fields are receipt assertions, not trusted route policy facts.
use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    MatrixPlanningEffectStore, MatrixPlanningSelectionStore, ModelRouteSelectionRead,
};
use tect_domain::{
    Error, MatrixDispositionDecision, ModelRouteContextAuthority, ModelRouteFact,
    ModelRouteFactProvenance, ModelRouteSelectionLink, ModelRouteWorkContext, Result,
    SliceCandidateNode,
};
use uuid::Uuid;

use crate::{
    matrix_planning_selection_store::current_context_evaluation, storage_error, store::PgUnitOfWork,
};

fn caller_assertion<T>(
    value: Option<T>,
    field: &str,
    candidate_set_id: Uuid,
    caller_request_id: Uuid,
    draft_index: usize,
    work_node_id: Uuid,
    work_node_revision: i64,
) -> ModelRouteFact<T> {
    match value {
        Some(value) => ModelRouteFact::Known {
            value,
            provenance: ModelRouteFactProvenance::Caller {
                source_ref: format!(
                    "native_planning_receipt/{candidate_set_id}/{caller_request_id}#/draft/nodes/{draft_index}/model_route_facts/{field}"
                ),
                work_node_id,
                work_node_revision,
            },
        },
        None => ModelRouteFact::Unknown,
    }
}

fn exact_work_context(
    snapshot: tect_application::MatrixPlanningEffectSnapshot,
    disposition_id: Uuid,
    mapped_work_node_id: Uuid,
    mapped_work_node_revision: i64,
    workspace_id: Uuid,
    request: &Value,
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
    let selected = matching[0];
    let SliceCandidateNode::Work {
        model_route_facts, ..
    } = &selected.body
    else {
        return Err(Error::StaleContext);
    };
    let submitted = request
        .pointer(&format!("/draft/nodes/{}", selected.draft_index))
        .ok_or(Error::StaleContext)?;
    if submitted.get("kind") != Some(&serde_json::json!("work"))
        || submitted.get("model_route_facts")
            != model_route_facts
                .as_ref()
                .map(|facts| serde_json::to_value(facts).map_err(storage_error))
                .transpose()?
                .as_ref()
    {
        return Err(Error::StaleContext);
    }
    if let Some(facts) = model_route_facts {
        facts.validate().map_err(|_| Error::StaleContext)?;
    }
    let context_authority =
        snapshot
            .link
            .context_provenance
            .as_ref()
            .map(|provenance| ModelRouteContextAuthority {
                frozen_snapshot_id: provenance.frozen_snapshot_id,
                authority_schema: provenance.authority_schema.clone(),
                requirements_semantic_digest: provenance.requirements_semantic_digest.clone(),
                operating_verification_digest: snapshot
                    .link
                    .selection
                    .expected_verification_digest
                    .clone(),
            });
    macro_rules! asserted {
        ($value:expr, $field:literal) => {
            caller_assertion(
                $value,
                $field,
                material.candidate_set_id,
                material.caller_request_id,
                selected.draft_index,
                mapped_work_node_id,
                mapped_work_node_revision,
            )
        };
    }
    let work = ModelRouteWorkContext {
        approved_matrix_selection: snapshot.link.selection,
        selection_link: ModelRouteSelectionLink {
            candidate_set_id: material.candidate_set_id,
            caller_request_id: material.caller_request_id,
            mapped_draft_node_index: selected.draft_index,
            mapped_work_node_id,
            mapped_work_node_revision,
        },
        context_authority,
        role: asserted!(
            model_route_facts
                .as_ref()
                .and_then(|facts| facts.role.clone()),
            "role"
        ),
        tool: asserted!(
            model_route_facts
                .as_ref()
                .and_then(|facts| facts.tool.clone()),
            "tool"
        ),
        data_class: asserted!(
            model_route_facts
                .as_ref()
                .and_then(|facts| facts.data_class.clone()),
            "data_class"
        ),
        host_capabilities: ModelRouteFact::Unknown,
        remaining_budget_units: asserted!(
            model_route_facts
                .as_ref()
                .and_then(|facts| facts.remaining_budget_units),
            "remaining_budget_units"
        ),
        available_latency_ms: asserted!(
            model_route_facts
                .as_ref()
                .and_then(|facts| facts.available_latency_ms),
            "available_latency_ms"
        ),
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
                true,
            )
            .await?
        else {
            return Ok(None);
        };
        let link = &snapshot.link;
        if link.selection.disposition_id != disposition_id {
            return Err(Error::StaleContext);
        }
        if link.context_provenance.is_some() {
            let (evaluation_digest, catalogue_version) =
                current_context_evaluation(self, workspace_id, link).await?;
            if evaluation_digest != link.evaluation_digest
                || catalogue_version != link.catalogue_version
            {
                return Err(Error::StaleContext);
            }
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
            &request,
        )
        .map(Some)
    }
}

#[cfg(test)]
mod tests;
