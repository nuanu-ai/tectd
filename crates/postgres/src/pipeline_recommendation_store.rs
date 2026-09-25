use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    AdvisoryStore, MatrixPlanningEffectSnapshot, MatrixPlanningEffectStore, MatrixTaskStore,
    MatrixVerificationStore, PipelineDispositionBasis, PipelineRecommendationBasis,
    PipelineRecommendationContext, PipelineRecommendationStore, PreparedPipelineRecommendation,
    pipeline_recommendation_source_digest,
};
use tect_domain::{
    ADVISORY_POLICY_VERSION, AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryOpportunityInput,
    AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference, Error,
    MatrixPlanningEffectMaterial, MatrixPlanningEffectNode, OwnerReportedEngineeringMatrixFacts,
    PipelineCatalogueSnapshot, PipelineDispositionResult, PipelineMatrixBasis,
    PipelineRecommendationManifest, PipelineRecommendationSource, Result, SliceCandidateNode,
    compose_independently_verified_owner_matrix, evaluate_matrix_verification,
    matrix_verified_disposition_digest,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()) {
        Some(code) if code == "42501" => Error::Forbidden,
        Some(code) if code == "23505" || code == "23514" => Error::InputConflict,
        _ => storage_error(error),
    }
}

// A ready review advances the set revision without rewriting the exact saved
// draft that the independent verifier attested. The SQL path below establishes
// that this is still the latest draft and that the current ready review loaded
// it. Rebuild the same canonical effect material without asserting that the
// draft revision equals the (later) review revision.
fn reviewed_effect_digest(
    snapshot: &MatrixPlanningEffectSnapshot,
    workspace_id: Uuid,
    latest_draft_revision: i64,
    ready_set_revision: i64,
) -> Result<String> {
    let link = &snapshot.link;
    link.selection.validate().map_err(|_| Error::StaleContext)?;
    if !snapshot.receipt_present
        || snapshot.current_result_revision != ready_set_revision
        || link.result_revision != latest_draft_revision
        || snapshot.selected_choice.candidate_id != link.selection.selected_choice_id
        || snapshot.saved_nodes.len() != link.mapped_nodes.len()
        || snapshot.saved_nodes.is_empty()
        || snapshot.matrix_owner_principal_id.is_nil()
        || link.selection.mapped_draft_node_indices
            != link
                .mapped_nodes
                .iter()
                .map(|node| node.draft_index)
                .collect::<Vec<_>>()
    {
        return Err(Error::StaleContext);
    }
    let nodes = link
        .mapped_nodes
        .iter()
        .zip(&snapshot.saved_nodes)
        .map(|(mapped, body)| {
            if mapped.node_id != body.id() || mapped.node_revision != body.revision() {
                return Err(Error::StaleContext);
            }
            Ok(MatrixPlanningEffectNode {
                draft_index: mapped.draft_index,
                node_id: mapped.node_id,
                node_revision: mapped.node_revision,
                body: body.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    MatrixPlanningEffectMaterial {
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
        matrix_owner_principal_id: snapshot.matrix_owner_principal_id,
        selected_choice: snapshot.selected_choice.clone(),
        nodes,
    }
    .canonical_digest()
}

#[async_trait]
impl PipelineRecommendationStore for PgUnitOfWork {
    async fn pipeline_disposition_by_opportunity(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<PipelineDispositionResult>> {
        crate::pipeline_disposition_store::by_opportunity(self, workspace_id, opportunity_id).await
    }

    async fn load_pipeline_disposition_basis(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<PipelineDispositionBasis>> {
        crate::pipeline_disposition_store::load_basis(self, workspace_id, opportunity_id).await
    }

    async fn pipeline_disposition_is_current(
        &mut self,
        workspace_id: Uuid,
        basis: &PipelineDispositionBasis,
    ) -> Result<bool> {
        crate::pipeline_disposition_store::is_current(self, workspace_id, basis).await
    }

    async fn capture_pipeline_disposition(
        &mut self,
        workspace_id: Uuid,
        result: &PipelineDispositionResult,
    ) -> Result<PipelineDispositionResult> {
        crate::pipeline_disposition_store::capture(self, workspace_id, result).await
    }

    async fn pipeline_recommendation_by_opportunity(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<PreparedPipelineRecommendation>> {
        let tenant = self.tenant_id()?;
        let request_key: Option<String> = sqlx::query_scalar(
            "SELECT request_key FROM advisory_opportunity \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
               AND capability='pipeline_recommendation'",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(opportunity_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let Some(request_key) = request_key else {
            return Ok(None);
        };
        let loaded = self
            .pipeline_recommendation_by_request(workspace_id, &request_key)
            .await?;
        if loaded
            .as_ref()
            .is_some_and(|saved| saved.opportunity.id != opportunity_id)
        {
            return Err(Error::InputConflict);
        }
        Ok(loaded)
    }

    async fn pipeline_recommendation_is_current(
        &mut self,
        workspace_id: Uuid,
        saved: &PreparedPipelineRecommendation,
    ) -> Result<bool> {
        // The original receipt is a lookup key, not authority. The opportunity
        // may advance to awaiting_response after the send, so compare its
        // immutable capture fields while allowing that lifecycle transition.
        let persisted = self
            .pipeline_recommendation_by_opportunity(workspace_id, saved.opportunity.id)
            .await?;
        let Some(persisted) = persisted else {
            return Ok(false);
        };
        let old = &saved.opportunity;
        let now = &persisted.opportunity;
        if saved.context != persisted.context
            || saved.manifest != persisted.manifest
            || old.workspace_id != workspace_id
            || old.state != AdvisoryOpportunityState::Prepared
            || old.primary_reason != AdvisoryReason::RecommendationPrepared
            || !matches!(
                now.state,
                AdvisoryOpportunityState::Prepared | AdvisoryOpportunityState::AwaitingResponse
            )
            || old.id != now.id
            || old.workspace_id != now.workspace_id
            || old.session_id != now.session_id
            || old.authorized_actor_id != now.authorized_actor_id
            || old.capability != now.capability
            || old.decision_point != now.decision_point
            || old.decision_point_version != now.decision_point_version
            || old.workflow_occurrence_key != now.workflow_occurrence_key
            || old.target_kind != now.target_kind
            || old.target_id != now.target_id
            || old.work_revision != now.work_revision
            || old.matrix_task_revision != now.matrix_task_revision
            || old.matrix_choice_set_digest != now.matrix_choice_set_digest
            || old.matrix_verification_digest != now.matrix_verification_digest
            || old.source_ref != now.source_ref
            || old.session_preference != now.session_preference
            || old.request_preference != now.request_preference
            || old.config_revision != now.config_revision
            || old.material_digest != now.material_digest
            || now.capability != AdvisoryCapability::PipelineRecommendation
            || now.decision_point != AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
            || saved.manifest.validate_digest().is_err()
            || old.material_digest != saved.manifest.digest
            || saved.context.verification_contract_digest != saved.manifest.digest
            || saved.context.work_node_id != saved.manifest.work_id
            || saved.context.work_node_revision != saved.manifest.work_revision
            || saved.context.catalogue_revision != saved.manifest.catalogue_revision
            || saved.context.catalogue_digest != saved.manifest.catalogue_digest
            || saved.context.eligible_kind_ids
                != saved
                    .manifest
                    .options
                    .iter()
                    .map(|option| option.id.clone())
                    .collect::<Vec<_>>()
        {
            return Ok(false);
        }

        let tenant = self.tenant_id()?;
        let mut config_query = String::from(
            "SELECT revision,mode,provider_profile_ref IS NOT NULL AS provider_configured, \
                    model_configuration IS NOT NULL AS model_configured \
             FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2",
        );
        let for_update = self.is_read_write();
        if for_update {
            config_query.push_str(" FOR SHARE");
        }
        let config: Option<(i64, String, bool, bool)> = sqlx::query_as(&config_query)
            .bind(tenant)
            .bind(workspace_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        if !config.is_some_and(|(revision, mode, provider, model)| {
            revision == old.config_revision && mode == "optional" && provider && model
        }) || !saved.manifest.should_call()
        {
            return Ok(false);
        }

        // The writer locks the mutable heads through provider-send admission;
        // the post-seal read-only transaction has a repeatable-read snapshot.
        // The DB dispatch guard independently rechecks the same path at INSERT.
        let basis = match self
            .load_pipeline_recommendation_basis(
                workspace_id,
                saved.context.candidate_set_id,
                saved.context.work_node_id,
                for_update,
            )
            .await
        {
            Ok(Some(basis)) => basis,
            Ok(None) | Err(Error::StaleContext) => return Ok(false),
            Err(error) => return Err(error),
        };
        let context = &saved.context;
        let manifest = &saved.manifest;
        let source = &basis.source;
        Ok(basis.scope_id == context.scope_id
            && basis.candidate_set_id == context.candidate_set_id
            && basis.candidate_set_revision == context.candidate_set_revision
            && basis.planning_snapshot_id == context.planning_snapshot_id
            && basis.source_snapshot_id == context.source_snapshot_id
            && basis.source_candidate_set_revision.to_string() == context.source_snapshot_revision
            && pipeline_recommendation_source_digest(
                basis.source_snapshot_id,
                basis.source_candidate_set_revision,
                &basis.selected_sources_digest,
            )? == context.source_snapshot_digest
            && source.work.revision() == context.work_node_revision
            && basis.matrix_disposition_id == context.matrix_disposition_id
            && basis.match_effect_attestation_id == context.match_effect_attestation_id
            && source.catalogue.revision == context.catalogue_revision
            && source.catalogue.digest == context.catalogue_digest
            && manifest.matrix_task_id == source.matrix.composition.task_id
            && manifest.matrix_task_revision == source.matrix.composition.task_revision
            && manifest.selected_choice_id == source.matrix.selected_choice_id
            && manifest.matrix_choice_set_digest == source.matrix.choice_set_digest
            && manifest.matrix_verification_digest == source.matrix.verification_digest
            && manifest.mandatory_card_ids == source.matrix.saved_mandatory_card_ids)
    }

    async fn load_pipeline_recommendation_basis(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        work_node_id: Uuid,
        for_update: bool,
    ) -> Result<Option<PipelineRecommendationBasis>> {
        let tenant = self.tenant_id()?;
        let mut query = String::from(
            "SELECT c.scope_id,c.revision AS set_revision,s.id AS planning_snapshot_id, \
                    s.source_snapshot_id,s.source_candidate_set_revision, \
                    source.selected_sources_digest,s.catalogue, \
                    draft.set_revision AS draft_revision, \
                    l.caller_request_id,l.disposition_id,l.task_id,l.task_revision, \
                    l.selected_choice_id,l.input_digest,l.choice_set_digest, \
                    l.verification_digest,l.evaluation_digest,l.catalogue_version, \
                    a.id AS attestation_id,a.effect_digest,a.verifier_principal_id, \
                    a.verdict \
             FROM slice_candidate_sets c \
             JOIN native_scopes n ON (n.tenant_id,n.workspace_id,n.id)= \
                  (c.tenant_id,c.workspace_id,c.scope_id) \
             JOIN slice_planning_snapshots s ON \
                  (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)= \
                  (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
             JOIN scope_candidate_sets source_set ON \
                  (source_set.tenant_id,source_set.workspace_id,source_set.id)= \
                  (n.tenant_id,n.workspace_id,n.source_candidate_set_id) \
             JOIN scope_candidate_snapshots source ON \
                  (source.tenant_id,source.workspace_id,source.candidate_set_id,source.id)= \
                  (source_set.tenant_id,source_set.workspace_id,source_set.id,s.source_snapshot_id) \
             JOIN slice_candidate_drafts draft ON \
                  (draft.tenant_id,draft.workspace_id,draft.candidate_set_id)= \
                  (c.tenant_id,c.workspace_id,c.id) \
             JOIN slice_candidate_reviews review ON \
                  (review.tenant_id,review.workspace_id,review.candidate_set_id,review.set_revision)= \
                  (c.tenant_id,c.workspace_id,c.id,c.revision) \
             JOIN matrix_planning_effect_attestations a ON \
                  (a.tenant_id,a.workspace_id,a.candidate_set_id,a.result_revision)= \
                  (draft.tenant_id,draft.workspace_id,draft.candidate_set_id,draft.set_revision) \
             JOIN matrix_planning_selection_links l ON \
                  (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)= \
                  (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id) \
             JOIN native_planning_receipts receipt ON \
                  (receipt.tenant_id,receipt.workspace_id,receipt.entity_id, \
                   receipt.operation,receipt.request_id)= \
                  (l.tenant_id,l.workspace_id,l.candidate_set_id, \
                   l.operation,l.caller_request_id) \
             JOIN advisory_matrix_disposition d ON \
                  (d.tenant_id,d.workspace_id,d.disposition_id)= \
                  (l.tenant_id,l.workspace_id,l.disposition_id) \
             WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 \
               AND c.status='ready' AND c.latest_input=s.planning_latest_input \
               AND n.revision=s.scope_revision AND NOT n.payload_erased \
               AND n.source_snapshot_id=s.source_snapshot_id \
               AND source_set.current_snapshot_id=source.id \
               AND source_set.revision=s.source_candidate_set_revision \
               AND draft.set_revision=(SELECT MAX(latest.set_revision) \
                    FROM slice_candidate_drafts latest \
                    WHERE latest.tenant_id=c.tenant_id \
                      AND latest.workspace_id=c.workspace_id \
                      AND latest.candidate_set_id=c.id) \
               AND draft.set_revision < c.revision \
               AND NOT draft.payload_erased AND draft.payload IS NOT NULL \
               AND NOT review.payload_erased AND review.payload IS NOT NULL \
               AND review.payload->>'verdict'='ready' \
               AND review.payload->>'revision'=c.revision::text \
               AND NOT receipt.payload_erased \
               AND receipt.request_payload IS NOT NULL \
               AND receipt.result_payload IS NOT NULL \
               AND receipt.result_payload->'draft'=draft.payload \
               AND receipt.result_payload#>>'{candidate_set,revision}'=draft.set_revision::text \
               AND a.verdict='match' AND l.scope_id=c.scope_id \
               AND l.result_revision=draft.set_revision \
               AND d.outcome='selected' AND d.selected_choice_id=l.selected_choice_id \
               AND d.task_id=l.task_id AND d.matrix_task_revision=l.task_revision \
               AND NOT EXISTS (SELECT 1 FROM native_slices opened \
                   WHERE opened.tenant_id=c.tenant_id AND opened.workspace_id=c.workspace_id \
                     AND opened.scope_id=c.scope_id AND opened.candidate_id=$4) \
             ORDER BY a.verified_at DESC,a.id DESC LIMIT 1",
        );
        if for_update {
            // The source snapshot and Matrix evidence are read-only to the runtime
            // role. Lock the mutable scope/source heads that the INSERT trigger
            // does not recheck; the trigger locks and rechecks draft/effect state.
            query.push_str(" FOR SHARE OF c,n,s,source_set");
        }
        let rows = sqlx::query(&query)
            .bind(tenant)
            .bind(workspace_id)
            .bind(candidate_set_id)
            .bind(work_node_id)
            .fetch_all(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };
        let caller_request_id: Uuid = row.try_get("caller_request_id").map_err(storage_error)?;
        let snapshot = self
            .matrix_planning_effect_snapshot(
                workspace_id,
                candidate_set_id,
                caller_request_id,
                for_update,
            )
            .await?
            .ok_or(Error::StaleContext)?;
        let attestation_digest: String = row.try_get("effect_digest").map_err(storage_error)?;
        let attestation_verifier: Uuid = row
            .try_get("verifier_principal_id")
            .map_err(storage_error)?;
        let draft_revision: i64 = row.try_get("draft_revision").map_err(storage_error)?;
        let ready_revision: i64 = row.try_get("set_revision").map_err(storage_error)?;
        if reviewed_effect_digest(&snapshot, workspace_id, draft_revision, ready_revision)?
            != attestation_digest
            || attestation_verifier == snapshot.link.caller_principal_id
            || attestation_verifier == snapshot.matrix_owner_principal_id
            || snapshot.link.selection.disposition_id
                != row
                    .try_get::<Uuid, _>("disposition_id")
                    .map_err(storage_error)?
        {
            return Err(Error::StaleContext);
        }
        let work = snapshot
            .saved_nodes
            .iter()
            .find(|node| node.id() == work_node_id)
            .cloned()
            .ok_or(Error::StaleContext)?;
        let SliceCandidateNode::Work {
            revision: work_revision,
            ..
        } = &work
        else {
            return Err(Error::StaleContext);
        };
        let work_revision = *work_revision;
        let task_id: Uuid = row.try_get("task_id").map_err(storage_error)?;
        let task = if for_update {
            self.lock_matrix_task(workspace_id, task_id).await?
        } else {
            self.matrix_task(workspace_id, task_id).await?
        }
        .ok_or(Error::StaleContext)?;
        let task_revision: i64 = row.try_get("task_revision").map_err(storage_error)?;
        let input_digest: String = row.try_get("input_digest").map_err(storage_error)?;
        let choice_set_digest: String = row.try_get("choice_set_digest").map_err(storage_error)?;
        let verification_digest: String =
            row.try_get("verification_digest").map_err(storage_error)?;
        if task.revision != task_revision
            || task.input_digest != input_digest
            || task.choice_set_digest.as_deref() != Some(choice_set_digest.as_str())
        {
            return Err(Error::StaleContext);
        }
        let verification = self
            .matrix_verification_for_revision(workspace_id, task_id, task_revision, &input_digest)
            .await?
            .ok_or(Error::StaleContext)?;
        if verification.digest != verification_digest
            || verification.owner_principal != task.recorded_by_principal_id.to_string()
            || verification.verifier_principal == verification.owner_principal
        {
            return Err(Error::StaleContext);
        }
        let now: i64 = sqlx::query_scalar(
            "SELECT FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint",
        )
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let validated = evaluate_matrix_verification(
            &task_id.to_string(),
            &task_revision.to_string(),
            &task.input,
            &verification,
            now,
        )
        .map_err(|_| Error::StaleContext)?;
        let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
            task_id.to_string(),
            task_revision.to_string(),
            task.input.clone(),
        )
        .map_err(|_| Error::StaleContext)?;
        let composition = compose_independently_verified_owner_matrix(&reported, &validated)
            .map_err(|_| Error::StaleContext)?;
        let choice_set = task.choice_set.as_ref().ok_or(Error::StaleContext)?;
        if matrix_verified_disposition_digest(&task.input, &composition, choice_set, &validated)
            .map_err(|_| Error::StaleContext)?
            != row
                .try_get::<String, _>("evaluation_digest")
                .map_err(storage_error)?
            || composition.catalogue_version
                != row
                    .try_get::<String, _>("catalogue_version")
                    .map_err(storage_error)?
        {
            return Err(Error::StaleContext);
        }
        let selected_choice_id: String =
            row.try_get("selected_choice_id").map_err(storage_error)?;
        if !choice_set
            .candidates
            .iter()
            .any(|choice| choice.candidate_id == selected_choice_id)
        {
            return Err(Error::StaleContext);
        }
        let catalogue: PipelineCatalogueSnapshot = serde_json::from_value(
            row.try_get::<Value, _>("catalogue")
                .map_err(storage_error)?,
        )
        .map_err(|_| Error::StaleContext)?;
        catalogue.validate().map_err(|_| Error::StaleContext)?;
        let scope_id: Uuid = row.try_get("scope_id").map_err(storage_error)?;
        let saved_mandatory_card_ids = composition
            .mandatory_cards
            .iter()
            .map(|card| card.id.to_string())
            .collect();
        let source = PipelineRecommendationSource {
            work,
            current_work_revision: work_revision,
            matrix: PipelineMatrixBasis {
                composition,
                selected_choice_id: selected_choice_id.clone(),
                current_selected_choice_id: selected_choice_id,
                current_task_revision: task_revision.to_string(),
                choice_set_digest: choice_set_digest.clone(),
                current_choice_set_digest: choice_set_digest,
                verification_digest: verification_digest.clone(),
                current_verification_digest: verification_digest,
                saved_mandatory_card_ids,
            },
            catalogue,
            definitions: Vec::new(),
            evidence_refs: Vec::new(),
        };
        Ok(Some(PipelineRecommendationBasis {
            scope_id,
            candidate_set_id,
            candidate_set_revision: row.try_get("set_revision").map_err(storage_error)?,
            planning_snapshot_id: row.try_get("planning_snapshot_id").map_err(storage_error)?,
            source_snapshot_id: row.try_get("source_snapshot_id").map_err(storage_error)?,
            source_candidate_set_revision: row
                .try_get("source_candidate_set_revision")
                .map_err(storage_error)?,
            selected_sources_digest: row
                .try_get("selected_sources_digest")
                .map_err(storage_error)?,
            matrix_disposition_id: row.try_get("disposition_id").map_err(storage_error)?,
            match_effect_attestation_id: row.try_get("attestation_id").map_err(storage_error)?,
            source,
        }))
    }

    async fn pipeline_recommendation_by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<PreparedPipelineRecommendation>> {
        let Some(mut opportunity) = self
            .advisory_opportunity_by_request(workspace_id, request_key)
            .await?
        else {
            return Ok(None);
        };
        if opportunity.capability != AdvisoryCapability::PipelineRecommendation {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT o.scope_id AS scope_id,context.candidate_set_id,context.candidate_set_revision, \
                    context.planning_snapshot_id,context.source_snapshot_id, \
                    context.source_snapshot_digest,context.work_node_id,context.work_node_revision, \
                    context.matrix_disposition_id,context.match_effect_attestation_id, \
                    context.catalogue_revision,context.catalogue_digest,context.eligible_kind_ids, \
                    context.verification_contract_digest,context.manifest_payload, \
                    context.manifest_digest,o.source_revision \
             FROM pipeline_advice_contexts context \
             JOIN advisory_opportunity o ON \
                  (o.tenant_id,o.workspace_id,o.id)= \
                  (context.tenant_id,context.workspace_id,context.opportunity_id) \
             WHERE context.tenant_id=$1 AND context.workspace_id=$2 \
               AND context.opportunity_id=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(opportunity.id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?
        .ok_or(Error::InputConflict)?;
        let manifest: PipelineRecommendationManifest = serde_json::from_value(
            row.try_get::<Option<Value>, _>("manifest_payload")
                .map_err(storage_error)?
                .ok_or(Error::InputConflict)?,
        )
        .map_err(|_| Error::InputConflict)?;
        manifest.validate_digest()?;
        let source_snapshot_revision: String = row
            .try_get::<Option<String>, _>("source_revision")
            .map_err(storage_error)?
            .ok_or(Error::InputConflict)?;
        let context = PipelineRecommendationContext {
            scope_id: row.try_get("scope_id").map_err(storage_error)?,
            candidate_set_id: row.try_get("candidate_set_id").map_err(storage_error)?,
            candidate_set_revision: row
                .try_get("candidate_set_revision")
                .map_err(storage_error)?,
            planning_snapshot_id: row.try_get("planning_snapshot_id").map_err(storage_error)?,
            source_snapshot_id: row.try_get("source_snapshot_id").map_err(storage_error)?,
            source_snapshot_revision,
            source_snapshot_digest: row
                .try_get("source_snapshot_digest")
                .map_err(storage_error)?,
            work_node_id: row.try_get("work_node_id").map_err(storage_error)?,
            work_node_revision: row.try_get("work_node_revision").map_err(storage_error)?,
            matrix_disposition_id: row
                .try_get("matrix_disposition_id")
                .map_err(storage_error)?,
            match_effect_attestation_id: row
                .try_get("match_effect_attestation_id")
                .map_err(storage_error)?,
            catalogue_revision: row.try_get("catalogue_revision").map_err(storage_error)?,
            catalogue_digest: row.try_get("catalogue_digest").map_err(storage_error)?,
            eligible_kind_ids: row.try_get("eligible_kind_ids").map_err(storage_error)?,
            verification_contract_digest: row
                .try_get("verification_contract_digest")
                .map_err(storage_error)?,
        };
        let stored_digest: Option<String> =
            row.try_get("manifest_digest").map_err(storage_error)?;
        if stored_digest.as_deref() != Some(manifest.digest.as_str())
            || context.verification_contract_digest != manifest.digest
            || opportunity.material_digest != manifest.digest
        {
            return Err(Error::InputConflict);
        }
        opportunity.work_revision = Some(context.work_node_revision);
        opportunity.source_ref = Some(context.source_snapshot_id.to_string());
        Ok(Some(PreparedPipelineRecommendation {
            opportunity,
            context,
            manifest,
        }))
    }

    async fn capture_pipeline_recommendation(
        &mut self,
        workspace_id: Uuid,
        input: &AdvisoryOpportunityInput,
        context: &PipelineRecommendationContext,
        manifest: &PipelineRecommendationManifest,
    ) -> Result<PreparedPipelineRecommendation> {
        input.validate()?;
        manifest.validate_digest()?;
        let actor = self.principal_id()?;
        if input.capability != AdvisoryCapability::PipelineRecommendation
            || input.decision_point != AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
            || input.authorized_actor_id != actor
            || input.target_id != Some(context.work_node_id)
            || input.work_revision != Some(context.work_node_revision)
            || input.material_digest != manifest.digest
            || context.verification_contract_digest != manifest.digest
            || manifest.work_id != context.work_node_id
            || manifest.work_revision != context.work_node_revision
            || manifest.catalogue_revision != context.catalogue_revision
            || manifest.catalogue_digest != context.catalogue_digest
            || context.eligible_kind_ids
                != manifest
                    .options
                    .iter()
                    .map(|o| o.id.clone())
                    .collect::<Vec<_>>()
        {
            return Err(Error::InputConflict);
        }
        if let Some(prior) = self
            .pipeline_recommendation_by_request(workspace_id, &input.workflow_occurrence_key)
            .await?
        {
            return if prior.context == *context
                && prior.manifest == *manifest
                && prior.opportunity.session_id == input.session_id
                && prior.opportunity.authorized_actor_id == input.authorized_actor_id
                && prior.opportunity.session_preference == input.session_preference
                && prior.opportunity.request_preference == input.request_preference
                && prior.opportunity.config_revision == input.config_revision
                && prior.opportunity.state == input.state
                && prior.opportunity.primary_reason == input.primary_reason
            {
                Ok(prior)
            } else {
                Err(Error::InputConflict)
            };
        }
        let current = self
            .load_pipeline_recommendation_basis(
                workspace_id,
                context.candidate_set_id,
                context.work_node_id,
                true,
            )
            .await?
            .ok_or(Error::StaleContext)?;
        if current.scope_id != context.scope_id
            || current.candidate_set_revision != context.candidate_set_revision
            || current.planning_snapshot_id != context.planning_snapshot_id
            || current.source_snapshot_id != context.source_snapshot_id
            || current.source_candidate_set_revision.to_string() != context.source_snapshot_revision
            || pipeline_recommendation_source_digest(
                current.source_snapshot_id,
                current.source_candidate_set_revision,
                &current.selected_sources_digest,
            )? != context.source_snapshot_digest
            || current.source.work.revision() != context.work_node_revision
            || current.matrix_disposition_id != context.matrix_disposition_id
            || current.match_effect_attestation_id != context.match_effect_attestation_id
            || current.source.catalogue.revision != context.catalogue_revision
            || current.source.catalogue.digest != context.catalogue_digest
            || manifest.matrix_task_id != current.source.matrix.composition.task_id
            || manifest.matrix_task_revision != current.source.matrix.composition.task_revision
            || manifest.selected_choice_id != current.source.matrix.selected_choice_id
            || manifest.matrix_choice_set_digest != current.source.matrix.choice_set_digest
            || manifest.matrix_verification_digest != current.source.matrix.verification_digest
            || manifest.mandatory_card_ids != current.source.matrix.saved_mandatory_card_ids
        {
            return Err(Error::StaleContext);
        }
        let tenant = self.tenant_id()?;
        let authorized: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM agent_sessions s \
             JOIN memberships m ON (m.tenant_id,m.workspace_id,m.principal_id)= \
                  (s.tenant_id,s.workspace_id,$4) \
             WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3 \
               AND public.tect_dk_session_principal(s.id)=$4 \
               AND public.tect_dk_is_owner($4))",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(input.session_id)
        .bind(actor)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if !authorized {
            return Err(Error::Forbidden);
        }
        sqlx::query(
            "INSERT INTO advisory_workspace_config_history \
                 (tenant_id,workspace_id,revision,previous_revision,mode, \
                  provider_profile_ref,model_configuration, \
                  changed_by_principal_id,changed_by_session_id) \
             VALUES ($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(actor)
        .bind(input.session_id)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        sqlx::query(
            "INSERT INTO advisory_workspace_config \
                 (tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration, \
                  updated_by_principal_id,updated_by_session_id) \
             VALUES ($1,$2,0,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(actor)
        .bind(input.session_id)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        let config: Option<(i64, String, Option<String>, Option<Value>)> = sqlx::query_as(
            "SELECT revision,mode,provider_profile_ref,model_configuration \
             FROM advisory_workspace_config \
             WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
        )
        .bind(tenant)
        .bind(workspace_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let (config_revision, mode, provider_profile_ref, model_configuration) =
            config.ok_or(Error::InternalInvariant)?;
        if config_revision != input.config_revision {
            return Err(Error::StaleRevision);
        }
        let required_no_call = if mode == "disabled" {
            Some(AdvisoryReason::WorkspaceDisabled)
        } else if input.session_preference == AdvisoryRequestPreference::Skip {
            Some(AdvisoryReason::SessionSkip)
        } else if input.request_preference == AdvisoryRequestPreference::Skip {
            Some(AdvisoryReason::RequestSkip)
        } else if input.primary_reason == AdvisoryReason::CapabilityUnavailable
            && input.state == AdvisoryOpportunityState::NoCall
        {
            Some(AdvisoryReason::CapabilityUnavailable)
        } else if manifest.options.is_empty() {
            Some(AdvisoryReason::ChoiceSetNotApplicable)
        } else if provider_profile_ref.is_none() || model_configuration.is_none() {
            Some(AdvisoryReason::ProviderUnconfigured)
        } else {
            None
        };
        if required_no_call.is_some_and(|reason| {
            input.state != AdvisoryOpportunityState::NoCall || input.primary_reason != reason
        }) || required_no_call.is_none()
            && (input.state != AdvisoryOpportunityState::Prepared
                || input.primary_reason != AdvisoryReason::RecommendationPrepared)
        {
            return Err(Error::InputConflict);
        }
        let opportunity_id = Uuid::new_v4();
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO advisory_opportunity \
                 (id,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id, \
                  session_id,authorized_actor_id,source_revision,capability,decision_point, \
                  config_revision,session_preference,request_preference,policy_version, \
                  request_key,material_digest,state,primary_reason) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19) \
             ON CONFLICT(tenant_id,workspace_id,request_key) DO NOTHING RETURNING id",
        )
        .bind(opportunity_id)
        .bind(tenant)
        .bind(workspace_id)
        .bind(context.scope_id)
        .bind(&input.target_kind)
        .bind(input.target_id)
        .bind(input.session_id)
        .bind(input.authorized_actor_id)
        .bind(&context.source_snapshot_revision)
        .bind(input.capability.as_str())
        .bind(input.decision_point.as_str())
        .bind(input.config_revision)
        .bind(input.session_preference.as_str())
        .bind(input.request_preference.as_str())
        .bind(ADVISORY_POLICY_VERSION)
        .bind(&input.workflow_occurrence_key)
        .bind(&input.material_digest)
        .bind(input.state.as_str())
        .bind(input.primary_reason.as_str())
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        if inserted.is_none() {
            let prior = self
                .pipeline_recommendation_by_request(workspace_id, &input.workflow_occurrence_key)
                .await?
                .ok_or(Error::InputConflict)?;
            return if prior.context == *context
                && prior.manifest == *manifest
                && prior.opportunity.session_id == input.session_id
                && prior.opportunity.authorized_actor_id == input.authorized_actor_id
            {
                Ok(prior)
            } else {
                Err(Error::InputConflict)
            };
        }
        sqlx::query(
            "INSERT INTO pipeline_advice_contexts \
                 (tenant_id,workspace_id,opportunity_id,candidate_set_id, \
                  candidate_set_revision,planning_snapshot_id,source_snapshot_id, \
                  work_node_id,work_node_revision,source_snapshot_digest, \
                  matrix_disposition_id,match_effect_attestation_id, \
                  catalogue_revision,catalogue_digest,eligible_kind_ids, \
                  verification_contract_digest,manifest_payload,manifest_digest) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(opportunity_id)
        .bind(context.candidate_set_id)
        .bind(context.candidate_set_revision)
        .bind(context.planning_snapshot_id)
        .bind(context.source_snapshot_id)
        .bind(context.work_node_id)
        .bind(context.work_node_revision)
        .bind(&context.source_snapshot_digest)
        .bind(context.matrix_disposition_id)
        .bind(context.match_effect_attestation_id)
        .bind(&context.catalogue_revision)
        .bind(&context.catalogue_digest)
        .bind(&context.eligible_kind_ids)
        .bind(&context.verification_contract_digest)
        .bind(serde_json::to_value(manifest).map_err(storage_error)?)
        .bind(&manifest.digest)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        self.pipeline_recommendation_by_request(workspace_id, &input.workflow_occurrence_key)
            .await?
            .ok_or(Error::InternalInvariant)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sql_line_continuations_preserve_token_boundaries() {
        for (line_number, line) in include_str!("pipeline_recommendation_store.rs")
            .lines()
            .enumerate()
        {
            if line.ends_with('\\') {
                assert!(
                    line.ends_with(" \\"),
                    "SQL line continuation needs a preceding space at line {}",
                    line_number + 1
                );
            }
        }
    }
}
