use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    ContextMatrixVerificationStore, MatrixDispositionRecord, MatrixPlanningMappedNode,
    MatrixPlanningSelectionLink, MatrixPlanningSelectionStore, MatrixRequirementsContextStore,
    MatrixTaskStore, context_matrix_verified_evaluation_digest,
};
use tect_domain::{
    Error, MATRIX_REQUIREMENTS_SCHEMA, MatrixDispositionDecision, MatrixPlanningContextProvenance,
    MatrixPlanningSelection, Result, compose_confirmed_requirements_matrix,
    evaluate_context_matrix_verification, resolve_matrix_requirements,
};
use uuid::Uuid;

use crate::{matrix_disposition_store::decode_disposition, storage_error, store::PgUnitOfWork};

/// Re-evaluate the V2 material while the task head and every declaration head
/// remain locked in this transaction. The caller's link is never authority.
pub(crate) async fn current_context_evaluation(
    tx: &mut PgUnitOfWork,
    workspace_id: Uuid,
    link: &MatrixPlanningSelectionLink,
) -> Result<(String, String)> {
    evaluate_current_context(tx, workspace_id, link, true).await
}

/// Verifier readback performs the same V2 task, declaration and effect checks
/// without owner-only writer locks. Its attestation path holds the parent
/// candidate-set lock and repeats this readback before insertion.
pub(crate) async fn current_context_evaluation_for_verifier(
    tx: &mut PgUnitOfWork,
    workspace_id: Uuid,
    link: &MatrixPlanningSelectionLink,
) -> Result<(String, String)> {
    evaluate_current_context(tx, workspace_id, link, false).await
}

async fn evaluate_current_context(
    tx: &mut PgUnitOfWork,
    workspace_id: Uuid,
    link: &MatrixPlanningSelectionLink,
    lock_authority: bool,
) -> Result<(String, String)> {
    let provenance = link
        .context_provenance
        .as_ref()
        .ok_or(Error::StaleContext)?;
    let source = tx
        .matrix_task_source(workspace_id, link.selection.task_id)
        .await?
        .ok_or(Error::StaleRevision)?;
    let binding = source.requirements_binding.ok_or(Error::StaleContext)?;
    if binding.snapshot_id != provenance.frozen_snapshot_id
        || binding.authority_schema != provenance.authority_schema
        || binding.semantic_digest != provenance.requirements_semantic_digest
        || binding.authority_schema != MATRIX_REQUIREMENTS_SCHEMA
    {
        return Err(Error::StaleContext);
    }
    let current = if lock_authority {
        tx.lock_matrix_task(workspace_id, link.selection.task_id)
            .await?
    } else {
        tx.matrix_task(workspace_id, link.selection.task_id).await?
    }
    .ok_or(Error::StaleRevision)?;
    if current != source.revision
        || current.revision != link.selection.task_revision
        || current.input_digest != link.selection.expected_input_digest
        || current.choice_set_digest.as_deref()
            != Some(link.selection.expected_choice_set_digest.as_str())
    {
        return Err(Error::StaleRevision);
    }
    let current_principal = tx.principal_id()?;
    let lineage = tx
        .matrix_requirements_lineage(workspace_id, current_principal, &binding.locator, false)
        .await
        .map_err(|_| Error::StaleContext)?;
    if lock_authority {
        for anchor in &lineage {
            tx.lock_matrix_requirements_head(workspace_id, *anchor)
                .await
                .map_err(|_| Error::StaleContext)?;
        }
    }
    let frozen = tx
        .frozen_matrix_requirements_by_id(workspace_id, binding.snapshot_id)
        .await?
        .ok_or(Error::StaleContext)?;
    if lineage.last().copied() != Some(frozen.anchor)
        || frozen.effective.schema() != binding.authority_schema
        || frozen.effective.semantic_digest() != binding.semantic_digest
    {
        return Err(Error::StaleContext);
    }
    let revisions = tx
        .matrix_requirements_revisions(workspace_id, &lineage)
        .await
        .map_err(|_| Error::StaleContext)?;
    let effective = resolve_matrix_requirements(&lineage, &revisions, MATRIX_REQUIREMENTS_SCHEMA)
        .map_err(|_| Error::StaleContext)?;
    if effective.semantic_digest() != binding.semantic_digest {
        return Err(Error::StaleContext);
    }
    let set = current.choice_set.as_ref().ok_or(Error::StaleContext)?;
    set.validate(&current.input)
        .map_err(|_| Error::StaleContext)?;
    if !set
        .candidates
        .iter()
        .any(|candidate| candidate.candidate_id == link.selection.selected_choice_id)
    {
        return Err(Error::StaleContext);
    }
    let record = tx
        .context_matrix_verification_for_revision(
            workspace_id,
            current.task_id,
            current.revision,
            &current.input_digest,
            binding.snapshot_id,
        )
        .await?
        .ok_or(Error::StaleContext)?;
    if record.digest != link.selection.expected_verification_digest
        || record.owner_principal != current.recorded_by_principal_id.to_string()
        || record.verifier_principal == record.owner_principal
        || record.input_digest != current.input_digest
        || record.frozen_snapshot_id != binding.snapshot_id.to_string()
        || record.authority_schema != binding.authority_schema
        || record.requirements_semantic_digest != binding.semantic_digest
    {
        return Err(Error::StaleContext);
    }
    let now: i64 = sqlx::query_scalar(
        "SELECT FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint",
    )
    .fetch_one(&mut **tx.transaction()?)
    .await
    .map_err(storage_error)?;
    let validated = evaluate_context_matrix_verification(
        &current.task_id.to_string(),
        &current.revision.to_string(),
        &binding.snapshot_id.to_string(),
        &current.input,
        &frozen.effective,
        &record,
        now,
    )
    .map_err(|_| Error::StaleContext)?;
    let composition = compose_confirmed_requirements_matrix(
        &current.task_id.to_string(),
        &current.revision.to_string(),
        &binding.snapshot_id.to_string(),
        &current.input,
        &frozen.effective,
        &validated,
        now,
    )
    .map_err(|_| Error::StaleContext)?;
    let digest =
        context_matrix_verified_evaluation_digest(&current.input, &composition, set, &record)
            .map_err(|_| Error::StaleContext)?;
    Ok((
        digest,
        composition.composition().catalogue_version.to_owned(),
    ))
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedMappedNode {
    draft_index: usize,
    node_id: Uuid,
    node_revision: i64,
}

fn mapped_nodes_json(nodes: &[MatrixPlanningMappedNode]) -> Result<Value> {
    let persisted: Vec<_> = nodes
        .iter()
        .map(|node| PersistedMappedNode {
            draft_index: node.draft_index,
            node_id: node.node_id,
            node_revision: node.node_revision,
        })
        .collect();
    serde_json::to_value(persisted).map_err(storage_error)
}

fn decode_mapped_nodes(value: Option<Value>) -> Result<Vec<MatrixPlanningMappedNode>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let persisted: Vec<PersistedMappedNode> =
        serde_json::from_value(value).map_err(storage_error)?;
    Ok(persisted
        .into_iter()
        .map(|node| MatrixPlanningMappedNode {
            draft_index: node.draft_index,
            node_id: node.node_id,
            node_revision: node.node_revision,
        })
        .collect())
}

fn link_write_error(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|c| c == "42501")
    {
        Error::Forbidden
    } else {
        storage_error(error)
    }
}

fn verify_mapped_nodes(
    request: &Value,
    result: &Value,
    link: &MatrixPlanningSelectionLink,
) -> Result<()> {
    let request_nodes = request
        .pointer("/draft/nodes")
        .and_then(Value::as_array)
        .ok_or(Error::InputConflict)?;
    let resolved_nodes = result
        .pointer("/draft/nodes")
        .and_then(Value::as_array)
        .ok_or(Error::InputConflict)?;
    if request_nodes.len() != resolved_nodes.len()
        || link.mapped_nodes.len() != link.selection.mapped_draft_node_indices.len()
        || link.mapped_nodes.is_empty()
    {
        return Err(Error::InputConflict);
    }
    for (mapped, index) in link
        .mapped_nodes
        .iter()
        .zip(&link.selection.mapped_draft_node_indices)
    {
        if mapped.draft_index != *index || mapped.node_id.is_nil() || mapped.node_revision < 1 {
            return Err(Error::InputConflict);
        }
        let submitted = request_nodes.get(*index).ok_or(Error::InputConflict)?;
        let resolved = resolved_nodes.get(*index).ok_or(Error::InputConflict)?;
        if submitted.get("kind") != resolved.get("kind")
            || submitted
                .get("kind")
                .and_then(Value::as_str)
                .is_none_or(|kind| kind != "work" && kind != "decision")
            || resolved.get("id") != Some(&serde_json::json!(mapped.node_id))
            || resolved.get("revision") != Some(&serde_json::json!(mapped.node_revision))
        {
            return Err(Error::InputConflict);
        }
        let identity = submitted.get("identity").ok_or(Error::InputConflict)?;
        if let Some(existing_id) = identity.get("candidate_id")
            && !existing_id.is_null()
            && existing_id != &serde_json::json!(mapped.node_id)
        {
            return Err(Error::InputConflict);
        }
    }
    Ok(())
}

#[async_trait]
impl MatrixPlanningSelectionStore for PgUnitOfWork {
    async fn matrix_planning_selection_link(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        caller_request_id: Uuid,
    ) -> Result<Option<MatrixPlanningSelectionLink>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT scope_id,disposition_id,task_id,task_revision,selected_choice_id, \
                    input_digest,choice_set_digest,verification_digest,evaluation_digest, \
                    catalogue_version,caller_principal_id,caller_session_id,result_revision,mapped_nodes, \
                    frozen_snapshot_id,authority_schema,requirements_semantic_digest \
             FROM matrix_planning_selection_links WHERE tenant_id=$1 AND workspace_id=$2 \
               AND candidate_set_id=$3 AND caller_request_id=$4",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(caller_request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(|row| {
            let mapped_json: Option<Value> = row.try_get("mapped_nodes").map_err(storage_error)?;
            let mapped_nodes = decode_mapped_nodes(mapped_json)?;
            let snapshot: Option<Uuid> =
                row.try_get("frozen_snapshot_id").map_err(storage_error)?;
            let context_provenance = snapshot
                .map(|frozen_snapshot_id| -> Result<_> {
                    Ok(MatrixPlanningContextProvenance {
                        frozen_snapshot_id,
                        authority_schema: row.try_get("authority_schema").map_err(storage_error)?,
                        requirements_semantic_digest: row
                            .try_get("requirements_semantic_digest")
                            .map_err(storage_error)?,
                    })
                })
                .transpose()?;
            Ok(MatrixPlanningSelectionLink {
                selection: MatrixPlanningSelection {
                    task_id: row.try_get("task_id").map_err(storage_error)?,
                    task_revision: row.try_get("task_revision").map_err(storage_error)?,
                    disposition_id: row.try_get("disposition_id").map_err(storage_error)?,
                    selected_choice_id: row.try_get("selected_choice_id").map_err(storage_error)?,
                    expected_input_digest: row.try_get("input_digest").map_err(storage_error)?,
                    expected_choice_set_digest: row
                        .try_get("choice_set_digest")
                        .map_err(storage_error)?,
                    expected_verification_digest: row
                        .try_get("verification_digest")
                        .map_err(storage_error)?,
                    mapped_draft_node_indices: mapped_nodes
                        .iter()
                        .map(|node| node.draft_index)
                        .collect(),
                },
                evaluation_digest: row.try_get("evaluation_digest").map_err(storage_error)?,
                context_provenance,
                catalogue_version: row.try_get("catalogue_version").map_err(storage_error)?,
                caller_principal_id: row.try_get("caller_principal_id").map_err(storage_error)?,
                caller_session_id: row.try_get("caller_session_id").map_err(storage_error)?,
                scope_id: row.try_get("scope_id").map_err(storage_error)?,
                candidate_set_id,
                caller_request_id,
                result_revision: row.try_get("result_revision").map_err(storage_error)?,
                mapped_nodes,
            })
        })
        .transpose()
    }

    async fn matrix_disposition_by_id(
        &mut self,
        workspace_id: Uuid,
        disposition_id: Uuid,
    ) -> Result<Option<MatrixDispositionRecord>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT d.disposition_id,d.request_id,d.actor_id,d.session_id,d.opportunity_id, \
                    d.task_id,d.matrix_task_revision,d.matrix_choice_set_digest,d.basis, \
                    d.advice_id,d.outcome,d.selected_choice_id,d.blocked_reason, \
                    r.input_digest,a.advice_digest \
             FROM advisory_matrix_disposition d \
             JOIN matrix_task_revisions r ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)= \
               (d.tenant_id,d.workspace_id,d.task_id,d.matrix_task_revision) \
             LEFT JOIN advisory_matrix_advice a ON (a.tenant_id,a.workspace_id,a.advice_id)= \
               (d.tenant_id,d.workspace_id,d.advice_id) \
             WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.disposition_id=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(disposition_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(decode_disposition).transpose()
    }

    async fn link_matrix_planning_selection(
        &mut self,
        workspace_id: Uuid,
        link: &MatrixPlanningSelectionLink,
    ) -> Result<()> {
        link.selection.validate()?;
        if workspace_id.is_nil()
            || link.scope_id.is_nil()
            || link.candidate_set_id.is_nil()
            || link.caller_request_id.is_nil()
            || link.caller_session_id.is_nil()
            || link.caller_principal_id != self.principal_id()?
            || link.result_revision < 1
        {
            return Err(Error::Forbidden);
        }
        if link.context_provenance.is_none() {
            return Err(Error::StaleContext);
        }
        let tenant = self.tenant_id()?;
        // This receipt is written by the existing native save before the link.
        // Lock it so erasure cannot change the payload before transaction commit.
        let receipt = sqlx::query(
            "SELECT request_payload,result_payload,payload_erased FROM native_planning_receipts \
             WHERE tenant_id=$1 AND workspace_id=$2 AND entity_id=$3 \
               AND operation='save_slice_draft' AND request_id=$4 FOR SHARE",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(link.candidate_set_id)
        .bind(link.caller_request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?
        .ok_or(Error::NotFound)?;
        let request: Option<serde_json::Value> =
            receipt.try_get("request_payload").map_err(storage_error)?;
        let result: Option<serde_json::Value> =
            receipt.try_get("result_payload").map_err(storage_error)?;
        let erased: bool = receipt.try_get("payload_erased").map_err(storage_error)?;
        if erased {
            return Err(Error::KnowledgePayloadErased);
        }
        let request = request.ok_or(Error::InternalInvariant)?;
        let result = result.ok_or(Error::InternalInvariant)?;
        let expected_selection = serde_json::to_value(&link.selection).map_err(storage_error)?;
        if request.get("matrix_selection") != Some(&expected_selection)
            || request.get("request_id") != Some(&serde_json::json!(link.caller_request_id))
            || request.get("candidate_set_id") != Some(&serde_json::json!(link.candidate_set_id))
            || request.get("scope_id") != Some(&serde_json::json!(link.scope_id))
            || result.pointer("/candidate_set/revision")
                != Some(&serde_json::json!(link.result_revision))
        {
            return Err(Error::InputConflict);
        }
        let stored_draft: Option<Value> = sqlx::query_scalar(
            "SELECT payload FROM slice_candidate_drafts \
             WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 \
               AND set_revision=$4 FOR SHARE",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(link.candidate_set_id)
        .bind(link.result_revision)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if stored_draft.as_ref() != result.get("draft") {
            return Err(Error::InputConflict);
        }
        verify_mapped_nodes(&request, &result, link)?;
        if let Some(prior) = self
            .matrix_planning_selection_link(
                workspace_id,
                link.candidate_set_id,
                link.caller_request_id,
            )
            .await?
        {
            return if prior == *link {
                Ok(())
            } else {
                Err(Error::InputConflict)
            };
        }
        let set_scope: Option<Uuid> = sqlx::query_scalar(
            "SELECT scope_id FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR SHARE",
        )
        .bind(tenant).bind(workspace_id).bind(link.candidate_set_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        if set_scope != Some(link.scope_id) {
            return Err(Error::StaleContext);
        }
        let disposition = self
            .matrix_disposition_by_id(workspace_id, link.selection.disposition_id)
            .await?
            .ok_or(Error::NotFound)?;
        if disposition.request.task_id != link.selection.task_id
            || disposition.request.expected_task_revision != link.selection.task_revision
            || disposition.request.expected_input_digest != link.selection.expected_input_digest
            || disposition.request.expected_choice_set_digest.as_deref()
                != Some(link.selection.expected_choice_set_digest.as_str())
            || !matches!(&disposition.request.decision,
                MatrixDispositionDecision::Selected { selected_choice_id } if selected_choice_id == &link.selection.selected_choice_id)
        {
            return Err(Error::InputConflict);
        }
        let (evaluation, catalogue_version) =
            current_context_evaluation(self, workspace_id, link).await?;
        if evaluation != link.evaluation_digest || catalogue_version != link.catalogue_version {
            return Err(Error::StaleContext);
        }
        let opportunity = sqlx::query(
            "SELECT o.work_item_kind,o.work_item_id,o.matrix_task_revision,o.matrix_choice_set_digest, \
                    o.matrix_verification_digest,o.material_digest,o.authorized_actor_id,o.session_id \
             FROM advisory_opportunity o WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3 FOR SHARE",
        )
        .bind(tenant).bind(workspace_id).bind(disposition.request.opportunity_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
        .ok_or(Error::StaleContext)?;
        if opportunity
            .try_get::<String, _>("work_item_kind")
            .map_err(storage_error)?
            != "matrix_task"
            || opportunity
                .try_get::<Option<Uuid>, _>("work_item_id")
                .map_err(storage_error)?
                != Some(link.selection.task_id)
            || opportunity
                .try_get::<Option<i64>, _>("matrix_task_revision")
                .map_err(storage_error)?
                != Some(link.selection.task_revision)
            || opportunity
                .try_get::<Option<String>, _>("matrix_choice_set_digest")
                .map_err(storage_error)?
                .as_deref()
                != Some(link.selection.expected_choice_set_digest.as_str())
            || opportunity
                .try_get::<Option<String>, _>("matrix_verification_digest")
                .map_err(storage_error)?
                .as_deref()
                != Some(link.selection.expected_verification_digest.as_str())
            || opportunity
                .try_get::<String, _>("material_digest")
                .map_err(storage_error)?
                != evaluation
            || opportunity
                .try_get::<Uuid, _>("authorized_actor_id")
                .map_err(storage_error)?
                != disposition.recorded_by_principal_id
            || opportunity
                .try_get::<Uuid, _>("session_id")
                .map_err(storage_error)?
                != disposition.recorded_by_session_id
        {
            return Err(Error::StaleContext);
        }
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO matrix_planning_selection_links \
              (tenant_id,workspace_id,candidate_set_id,caller_request_id,scope_id,disposition_id, \
               task_id,task_revision,selected_choice_id,input_digest,choice_set_digest, \
               verification_digest,evaluation_digest,catalogue_version,caller_principal_id,caller_session_id,result_revision,mapped_nodes, \
               frozen_snapshot_id,authority_schema,requirements_semantic_digest) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21) \
             ON CONFLICT DO NOTHING RETURNING caller_request_id",
        )
        .bind(tenant).bind(workspace_id).bind(link.candidate_set_id).bind(link.caller_request_id)
        .bind(link.scope_id).bind(link.selection.disposition_id).bind(link.selection.task_id)
        .bind(link.selection.task_revision).bind(&link.selection.selected_choice_id)
        .bind(&link.selection.expected_input_digest).bind(&link.selection.expected_choice_set_digest)
        .bind(&link.selection.expected_verification_digest).bind(&link.evaluation_digest)
        .bind(&link.catalogue_version).bind(link.caller_principal_id).bind(link.caller_session_id)
        .bind(link.result_revision)
        .bind(mapped_nodes_json(&link.mapped_nodes)?)
        .bind(link.context_provenance.as_ref().map(|p| p.frozen_snapshot_id))
        .bind(link.context_provenance.as_ref().map(|p| &p.authority_schema))
        .bind(link.context_provenance.as_ref().map(|p| &p.requirements_semantic_digest))
        .fetch_optional(&mut **self.transaction()?).await.map_err(link_write_error)?;
        if inserted.is_some() {
            return Ok(());
        }
        match self
            .matrix_planning_selection_link(
                workspace_id,
                link.candidate_set_id,
                link.caller_request_id,
            )
            .await?
        {
            Some(prior) if prior == *link => Ok(()),
            _ => Err(Error::InputConflict),
        }
    }
}

#[cfg(test)]
#[path = "matrix_planning_selection_store_tests.rs"]
mod tests;
