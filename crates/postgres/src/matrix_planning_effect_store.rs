use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    MatrixPlanningEffectAttestation, MatrixPlanningEffectSnapshot, MatrixPlanningEffectStore,
    MatrixPlanningEffectVerdict, MatrixPlanningSelectionLink, UnitOfWork,
};
use tect_domain::{
    EngineeringChoiceSet, Error, MatrixPlanningSelection, PrincipalRole,
    ResolvedSliceCandidateDraft, Result,
};
use uuid::Uuid;

use crate::{
    matrix_planning_selection_store::{
        current_context_evaluation, current_context_evaluation_for_verifier, decode_mapped_nodes,
        decode_provenance, require_workspace_reader, validate_persisted_link,
    },
    storage_error,
    store::PgUnitOfWork,
};

fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()) {
        Some(code) if code == "42501" => Error::Forbidden,
        Some(code) if code == "23505" => Error::InputConflict,
        _ => storage_error(error),
    }
}

fn missing_captured_choice(has_context_provenance: bool) -> Error {
    if has_context_provenance {
        Error::InternalInvariant
    } else {
        Error::StaleContext
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
        require_workspace_reader(self, workspace_id).await?;
        let tenant = self.tenant_id()?;
        if for_update && !self.is_read_write() {
            return Err(Error::Forbidden);
        }
        // Lock every mutable source of effect content through the INSERT.
        // The link and task revision are immutable, and the attestation trigger
        // locks and rechecks them with owner privileges at INSERT.
        let mut query = String::from(
            "SELECT l.scope_id,l.disposition_id,l.task_id,l.task_revision, \
                l.selected_choice_id,l.input_digest,l.choice_set_digest, \
                l.verification_digest,l.evaluation_digest,l.catalogue_version, \
                l.caller_principal_id,l.caller_session_id,l.result_revision,l.mapped_nodes, \
                l.frozen_snapshot_id,l.authority_schema,l.requirements_semantic_digest, \
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
        let context_provenance = decode_provenance(
            row.try_get("frozen_snapshot_id").map_err(storage_error)?,
            row.try_get("authority_schema").map_err(storage_error)?,
            row.try_get("requirements_semantic_digest")
                .map_err(storage_error)?,
        )?;
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
            context_provenance,
            catalogue_version: row.try_get("catalogue_version").map_err(storage_error)?,
            caller_principal_id: row.try_get("caller_principal_id").map_err(storage_error)?,
            caller_session_id: row.try_get("caller_session_id").map_err(storage_error)?,
            scope_id: row.try_get("scope_id").map_err(storage_error)?,
            candidate_set_id,
            caller_request_id,
            result_revision: row.try_get("result_revision").map_err(storage_error)?,
            mapped_nodes,
        };
        validate_persisted_link(&link)?;
        let choice_set: Value = row
            .try_get::<Option<Value>, _>("choice_set")
            .map_err(storage_error)?
            .ok_or_else(|| missing_captured_choice(link.context_provenance.is_some()))?;
        let choice_set: EngineeringChoiceSet =
            serde_json::from_value(choice_set).map_err(|_| Error::InternalInvariant)?;
        let selected_choice = choice_set
            .candidates
            .into_iter()
            .find(|choice| choice.candidate_id == link.selection.selected_choice_id)
            .ok_or_else(|| missing_captured_choice(link.context_provenance.is_some()))?;
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
        let evaluated = if for_update {
            current_context_evaluation(self, workspace_id, &link).await
        } else {
            current_context_evaluation_for_verifier(self, workspace_id, &link).await
        };
        let context_current = match evaluated {
            Ok((digest, catalogue)) => {
                digest == link.evaluation_digest && catalogue == link.catalogue_version
            }
            Err(Error::StaleContext | Error::StaleRevision) => false,
            Err(error) => return Err(error),
        };
        let is_current = context_current
            && current_result_revision == link.result_revision
            && current_scope_id == link.scope_id
            && stored_digest.as_deref() == Some(link.selection.expected_choice_set_digest.as_str())
            && receipt_result.as_ref().and_then(|v| v.get("draft")) == saved_draft.as_ref()
            && receipt_result
                .as_ref()
                .and_then(|v| v.pointer("/candidate_set/revision"))
                == Some(&serde_json::json!(link.result_revision));
        let saved_nodes = if let Some(draft) = saved_draft {
            let draft: ResolvedSliceCandidateDraft =
                serde_json::from_value(draft).map_err(|_| Error::InternalInvariant)?;
            link.mapped_nodes
                .iter()
                .map(|mapped| {
                    draft
                        .nodes
                        .get(mapped.draft_index)
                        .cloned()
                        .ok_or(Error::InternalInvariant)
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        if !saved_nodes.is_empty()
            && (saved_nodes.len() != link.mapped_nodes.len()
                || saved_nodes
                    .iter()
                    .zip(&link.mapped_nodes)
                    .any(|(node, mapped)| {
                        node.id() != mapped.node_id || node.revision() != mapped.node_revision
                    }))
        {
            return Err(Error::InternalInvariant);
        }
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
        require_workspace_reader(self, workspace_id).await?;
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
        native_session_id: &str,
        host_id: Uuid,
    ) -> Result<()> {
        if !self.is_read_write() || self.principal_role()? != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        // Chosen native key and authenticated host are checked again before replay.
        self.lock_native_session(host_id, native_session_id).await?;
        let session = self
            .session(host_id, native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        if session.revoked {
            return Err(Error::SessionRevoked);
        }
        if session.host_id != host_id
            || session.native_session_id != native_session_id
            || session.workspace_id != workspace_id
            || session.id != attestation.verifier_session_id
        {
            return Err(Error::SessionWorkspaceMismatch);
        }
        self.workspace(workspace_id).await?.ok_or(Error::NotFound)?;
        require_workspace_reader(self, workspace_id).await?;
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
        if snapshot.link.context_provenance.is_none() {
            return Err(Error::StaleContext);
        }
        let (evaluation_digest, catalogue_version) =
            current_context_evaluation(self, workspace_id, &snapshot.link).await?;
        if evaluation_digest != snapshot.link.evaluation_digest
            || catalogue_version != snapshot.link.catalogue_version
        {
            return Err(Error::StaleContext);
        }
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
        require_workspace_reader(self, workspace_id).await?;
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
    fn missing_captured_choice_is_corruption_only_for_v2() {
        assert_eq!(missing_captured_choice(true), Error::InternalInvariant);
        assert_eq!(missing_captured_choice(false), Error::StaleContext);
    }

    #[test]
    fn legacy_unmapped_link_cannot_supply_effect_material() {
        assert!(decode_mapped_nodes(None).unwrap().is_empty());
        assert!(matches!(
            decode_mapped_nodes(Some(serde_json::json!({"node_id": Uuid::new_v4()}))),
            Err(Error::InternalInvariant)
        ));
    }
}
