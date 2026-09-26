use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};
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
    PipelineCatalogueSnapshot, PipelineCompatibilityPolicy, PipelineDispositionResult,
    PipelineMatrixBasis, PipelineRecommendationManifest, PipelineRecommendationSource, Result,
    SliceCandidateNode, compose_independently_verified_owner_matrix, evaluate_matrix_verification,
    matrix_input_digest, matrix_verified_disposition_digest,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

mod basis;
mod current;
pub(crate) mod interpretation;
mod receipt;

fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()) {
        Some(code) if code == "42501" => Error::Forbidden,
        Some(code) if code == "23505" || code == "23514" => Error::InputConflict,
        _ => storage_error(error),
    }
}

fn selected_candidate_digest(source: &PipelineRecommendationSource) -> Result<String> {
    let selected = source
        .matrix
        .choice_set
        .candidates
        .iter()
        .find(|candidate| candidate.candidate_id == source.matrix.selected_choice_id)
        .ok_or(Error::StaleContext)?;
    let bytes = serde_json::to_vec(selected).map_err(|_| Error::StaleContext)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
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
    async fn insert_pipeline_advice_interpretation(
        &mut self,
        workspace_id: Uuid,
        value: &tect_application::PipelineAdviceInterpretation,
    ) -> Result<tect_application::PipelineAdviceInterpretation> {
        interpretation::insert(self, workspace_id, value).await
    }
    async fn pipeline_advice_interpretation(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<tect_application::PipelineAdviceInterpretation>> {
        interpretation::get(self, workspace_id, opportunity_id).await
    }
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
        current::pipeline_recommendation_by_opportunity(self, workspace_id, opportunity_id).await
    }

    async fn pipeline_recommendation_is_current(
        &mut self,
        workspace_id: Uuid,
        saved: &PreparedPipelineRecommendation,
    ) -> Result<bool> {
        current::pipeline_recommendation_is_current(self, workspace_id, saved).await
    }

    async fn load_pipeline_recommendation_basis(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        work_node_id: Uuid,
        for_update: bool,
    ) -> Result<Option<PipelineRecommendationBasis>> {
        basis::load_pipeline_recommendation_basis(
            self,
            workspace_id,
            candidate_set_id,
            work_node_id,
            for_update,
        )
        .await
    }

    async fn pipeline_recommendation_by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<PreparedPipelineRecommendation>> {
        receipt::pipeline_recommendation_by_request(self, workspace_id, request_key).await
    }

    async fn capture_pipeline_recommendation(
        &mut self,
        workspace_id: Uuid,
        input: &AdvisoryOpportunityInput,
        context: &PipelineRecommendationContext,
        manifest: &PipelineRecommendationManifest,
    ) -> Result<PreparedPipelineRecommendation> {
        receipt::capture_pipeline_recommendation(self, workspace_id, input, context, manifest).await
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn schema_three_plan_migration_is_forward_only_and_binds_open() {
        let migration = include_str!("../migrations/0072_pipeline_verification_plan_binding.sql");
        assert!(migration.contains("RENAME COLUMN eligible_kind_ids TO eligible_option_ids"));
        assert!(migration.contains("tect.pipeline-recommendation/3"));
        assert!(migration.contains("tect.pipeline-verification-plan/1"));
        assert!(migration.contains("NEW.verification_plan_bindings := bindings"));
        assert!(
            migration
                .contains("NEW.selected_option_id := NEW.result_payload->>'selected_option_id'")
        );
        assert!(migration.contains("NEW.verification_plan_id := disposition.verification_plan_id"));
        assert!(migration.contains("CREATE TRIGGER z_pipeline_slice_open_plan_binding"));
        assert!(migration.contains(
            "REVOKE ALL PRIVILEGES ON FUNCTION pipeline_slice_open_plan_binding() FROM PUBLIC"
        ));
        assert!(!migration.contains("DROP TABLE"));
    }

    #[test]
    fn sql_line_continuations_preserve_token_boundaries() {
        for source in [
            include_str!("pipeline_recommendation_store.rs"),
            include_str!("pipeline_recommendation_store/basis.rs"),
            include_str!("pipeline_recommendation_store/current.rs"),
            include_str!("pipeline_recommendation_store/receipt.rs"),
        ] {
            for (line_number, line) in source.lines().enumerate() {
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
}
