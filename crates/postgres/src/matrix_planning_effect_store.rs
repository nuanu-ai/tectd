use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    MatrixPlanningEffectAttestation, MatrixPlanningEffectSnapshot, MatrixPlanningEffectStore,
    MatrixPlanningEffectVerdict, MatrixPlanningMappedNode, MatrixPlanningSelectionLink,
};
use tect_domain::{
    EngineeringChoiceSet, Error, MatrixPlanningSelection, ResolvedSliceCandidateDraft, Result,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedMappedNode {
    draft_index: usize,
    node_id: Uuid,
    node_revision: i64,
}

fn decode_mapped_nodes(value: Option<Value>) -> Result<Vec<MatrixPlanningMappedNode>> {
    let Some(value) = value else {
        // Links predating mapped-node provenance are deliberately ineligible.
        return Ok(Vec::new());
    };
    let nodes: Vec<PersistedMappedNode> =
        serde_json::from_value(value).map_err(|_| Error::StaleContext)?;
    Ok(nodes
        .into_iter()
        .map(|node| MatrixPlanningMappedNode {
            draft_index: node.draft_index,
            node_id: node.node_id,
            node_revision: node.node_revision,
        })
        .collect())
}

fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()) {
        Some(code) if code == "42501" => Error::Forbidden,
        Some(code) if code == "23505" => Error::InputConflict,
        _ => storage_error(error),
    }
}

#[async_trait]
impl MatrixPlanningEffectStore for PgUnitOfWork {
    async fn matrix_planning_effect_snapshot(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        caller_request_id: Uuid,
        for_update: bool,
    ) -> Result<Option<MatrixPlanningEffectSnapshot>> {
        let tenant = self.tenant_id()?;
        // Lock every mutable source of effect content through the INSERT.
        // The link and task revision are immutable, and the attestation trigger
        // locks and rechecks them with owner privileges at INSERT.
        let mut query = String::from(
            "SELECT l.scope_id,l.disposition_id,l.task_id,l.task_revision, \
                l.selected_choice_id,l.input_digest,l.choice_set_digest, \
                l.verification_digest,l.evaluation_digest,l.catalogue_version, \
                l.caller_principal_id,l.caller_session_id,l.result_revision,l.mapped_nodes, \
                c.revision AS current_result_revision,c.scope_id AS current_scope_id, \
                r.recorded_by_principal_id,r.choice_set,r.choice_set_digest AS stored_choice_set_digest, \
                receipt.payload_erased,receipt.request_payload,receipt.result_payload, \
                draft.payload AS saved_draft \
             FROM matrix_planning_selection_links l \
             JOIN slice_candidate_sets c ON (c.tenant_id,c.workspace_id,c.id)= \
                 (l.tenant_id,l.workspace_id,l.candidate_set_id) \
             JOIN matrix_task_revisions r ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)= \
                 (l.tenant_id,l.workspace_id,l.task_id,l.task_revision) \
             LEFT JOIN native_planning_receipts receipt ON \
                 (receipt.tenant_id,receipt.workspace_id,receipt.entity_id,receipt.operation,receipt.request_id)= \
                 (l.tenant_id,l.workspace_id,l.candidate_set_id,l.operation,l.caller_request_id) \
             LEFT JOIN slice_candidate_drafts draft ON \
                 (draft.tenant_id,draft.workspace_id,draft.candidate_set_id,draft.set_revision)= \
                 (l.tenant_id,l.workspace_id,l.candidate_set_id,l.result_revision) \
             WHERE l.tenant_id=$1 AND l.workspace_id=$2 \
               AND l.candidate_set_id=$3 AND l.caller_request_id=$4",
        );
        if for_update {
            // A nullable side of an outer join cannot be row-locked in PG.
            // Missing receipt/draft is ineligible for a write in any case.
            query = query.replace(
                "LEFT JOIN native_planning_receipts",
                "JOIN native_planning_receipts",
            );
            query = query.replace(
                "LEFT JOIN slice_candidate_drafts",
                "JOIN slice_candidate_drafts",
            );
            query.push_str(" FOR SHARE OF c,receipt,draft");
        }
        let row = sqlx::query(&query)
            .bind(tenant)
            .bind(workspace_id)
            .bind(candidate_set_id)
            .bind(caller_request_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        let Some(row) = row else { return Ok(None) };
        let mapped_nodes =
            decode_mapped_nodes(row.try_get("mapped_nodes").map_err(storage_error)?)?;
        let link = MatrixPlanningSelectionLink {
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
            catalogue_version: row.try_get("catalogue_version").map_err(storage_error)?,
            caller_principal_id: row.try_get("caller_principal_id").map_err(storage_error)?,
            caller_session_id: row.try_get("caller_session_id").map_err(storage_error)?,
            scope_id: row.try_get("scope_id").map_err(storage_error)?,
            candidate_set_id,
            caller_request_id,
            result_revision: row.try_get("result_revision").map_err(storage_error)?,
            mapped_nodes,
        };
        let choice_set: Value = row
            .try_get::<Option<Value>, _>("choice_set")
            .map_err(storage_error)?
            .ok_or(Error::StaleContext)?;
        let choice_set: EngineeringChoiceSet =
            serde_json::from_value(choice_set).map_err(|_| Error::StaleContext)?;
        let selected_choice = choice_set
            .candidates
            .into_iter()
            .find(|choice| choice.candidate_id == link.selection.selected_choice_id)
            .ok_or(Error::StaleContext)?;
        let stored_digest: Option<String> = row
            .try_get("stored_choice_set_digest")
            .map_err(storage_error)?;
        let current_result_revision: i64 = row
            .try_get("current_result_revision")
            .map_err(storage_error)?;
        let current_scope_id: Uuid = row.try_get("current_scope_id").map_err(storage_error)?;
        let receipt_erased: Option<bool> = row.try_get("payload_erased").map_err(storage_error)?;
        let receipt_request: Option<Value> =
            row.try_get("request_payload").map_err(storage_error)?;
        let receipt_result: Option<Value> = row.try_get("result_payload").map_err(storage_error)?;
        let saved_draft: Option<Value> = row.try_get("saved_draft").map_err(storage_error)?;
        let receipt_present =
            receipt_erased == Some(false) && receipt_request.is_some() && receipt_result.is_some();
        let is_current = current_result_revision == link.result_revision
            && current_scope_id == link.scope_id
            && stored_digest.as_deref() == Some(link.selection.expected_choice_set_digest.as_str())
            && receipt_result.as_ref().and_then(|v| v.get("draft")) == saved_draft.as_ref()
            && receipt_result
                .as_ref()
                .and_then(|v| v.pointer("/candidate_set/revision"))
                == Some(&serde_json::json!(link.result_revision));
        let saved_nodes = if let Some(draft) = saved_draft {
            let draft: ResolvedSliceCandidateDraft =
                serde_json::from_value(draft).map_err(|_| Error::StaleContext)?;
            link.mapped_nodes
                .iter()
                .map(|mapped| {
                    draft
                        .nodes
                        .get(mapped.draft_index)
                        .cloned()
                        .ok_or(Error::StaleContext)
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        Ok(Some(MatrixPlanningEffectSnapshot {
            link,
            receipt_present,
            selected_choice,
            matrix_owner_principal_id: row
                .try_get("recorded_by_principal_id")
                .map_err(storage_error)?,
            saved_nodes,
            current_result_revision,
            is_current,
        }))
    }

    async fn matrix_planning_effect_attestation_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<MatrixPlanningEffectAttestation>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT candidate_set_id,caller_request_id,result_revision,effect_digest, \
                    verifier_principal_id,verifier_session_id,verdict,summary \
             FROM matrix_planning_effect_attestations \
             WHERE tenant_id=$1 AND workspace_id=$2 AND verifier_request_id=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(|row| {
            let verdict: String = row.try_get("verdict").map_err(storage_error)?;
            Ok(MatrixPlanningEffectAttestation {
                request_id,
                workspace_id,
                candidate_set_id: row.try_get("candidate_set_id").map_err(storage_error)?,
                caller_request_id: row.try_get("caller_request_id").map_err(storage_error)?,
                expected_result_revision: row.try_get("result_revision").map_err(storage_error)?,
                effect_digest: row.try_get("effect_digest").map_err(storage_error)?,
                verifier_principal_id: row
                    .try_get("verifier_principal_id")
                    .map_err(storage_error)?,
                verifier_session_id: row.try_get("verifier_session_id").map_err(storage_error)?,
                verdict: match verdict.as_str() {
                    "match" => MatrixPlanningEffectVerdict::Matches,
                    "reject" => MatrixPlanningEffectVerdict::Rejects,
                    _ => return Err(Error::InternalInvariant),
                },
                summary: row.try_get("summary").map_err(storage_error)?,
            })
        })
        .transpose()
    }

    async fn append_matrix_planning_effect_attestation(
        &mut self,
        workspace_id: Uuid,
        attestation: &MatrixPlanningEffectAttestation,
    ) -> Result<()> {
        if attestation.workspace_id != workspace_id
            || attestation.verifier_principal_id != self.principal_id()?
        {
            return Err(Error::Forbidden);
        }
        let snapshot = self
            .matrix_planning_effect_snapshot(
                workspace_id,
                attestation.candidate_set_id,
                attestation.caller_request_id,
                true,
            )
            .await?
            .ok_or(Error::NotFound)?;
        if snapshot.current_result_revision != attestation.expected_result_revision {
            return Err(Error::StaleRevision);
        }
        if snapshot.effect_digest(workspace_id)? != attestation.effect_digest {
            return Err(Error::InputConflict);
        }
        if attestation.verifier_principal_id == snapshot.link.caller_principal_id
            || attestation.verifier_principal_id == snapshot.matrix_owner_principal_id
        {
            return Err(Error::Forbidden);
        }
        if let Some(existing) = self
            .matrix_planning_effect_attestation_by_request(workspace_id, attestation.request_id)
            .await?
        {
            return if existing == *attestation {
                Ok(())
            } else {
                Err(Error::InputConflict)
            };
        }
        let tenant = self.tenant_id()?;
        let verdict = match attestation.verdict {
            MatrixPlanningEffectVerdict::Matches => "match",
            MatrixPlanningEffectVerdict::Rejects => "reject",
        };
        sqlx::query(
            "INSERT INTO matrix_planning_effect_attestations \
                (tenant_id,workspace_id,candidate_set_id,caller_request_id,result_revision, \
                 effect_digest,verifier_principal_id,verifier_session_id,verdict,summary,verifier_request_id) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(tenant).bind(workspace_id).bind(attestation.candidate_set_id)
        .bind(attestation.caller_request_id).bind(attestation.expected_result_revision)
        .bind(&attestation.effect_digest).bind(attestation.verifier_principal_id)
        .bind(attestation.verifier_session_id).bind(verdict).bind(&attestation.summary)
        .bind(attestation.request_id)
        .execute(&mut **self.transaction()?).await.map_err(write_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_unmapped_link_cannot_supply_effect_material() {
        assert!(decode_mapped_nodes(None).unwrap().is_empty());
        assert!(matches!(
            decode_mapped_nodes(Some(serde_json::json!({"node_id": Uuid::new_v4()}))),
            Err(Error::StaleContext)
        ));
    }
}
