use async_trait::async_trait;
use tect_domain::{
    AdvisoryOpportunity, AdvisoryOpportunityInput, PipelineDefinitionSnapshot, PipelineKind,
    PipelineRecommendationManifest, PipelineRecommendationSource, Result,
};
use uuid::Uuid;

/// All fields are read from one current saved planning and Matrix path. The
/// adapter must never derive the match from a public verifier projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineRecommendationBasis {
    pub scope_id: Uuid,
    pub candidate_set_id: Uuid,
    pub candidate_set_revision: i64,
    pub planning_snapshot_id: Uuid,
    pub source_snapshot_id: Uuid,
    pub source_candidate_set_revision: i64,
    pub selected_sources_digest: String,
    pub matrix_disposition_id: Uuid,
    pub match_effect_attestation_id: Uuid,
    /// Definitions are supplied by the pinned definition provider in the app.
    pub source: PipelineRecommendationSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineRecommendationContext {
    pub scope_id: Uuid,
    pub candidate_set_id: Uuid,
    pub candidate_set_revision: i64,
    pub planning_snapshot_id: Uuid,
    pub source_snapshot_id: Uuid,
    pub source_snapshot_revision: String,
    pub source_snapshot_digest: String,
    pub work_node_id: Uuid,
    pub work_node_revision: i64,
    pub matrix_disposition_id: Uuid,
    pub match_effect_attestation_id: Uuid,
    pub catalogue_revision: String,
    pub catalogue_digest: String,
    pub eligible_kind_ids: Vec<String>,
    pub verification_contract_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedPipelineRecommendation {
    pub opportunity: AdvisoryOpportunity,
    pub context: PipelineRecommendationContext,
    pub manifest: PipelineRecommendationManifest,
}

/// A missing or invalid definition is excluded from eligibility. The host
/// supplies immutable snapshots pinned to the current catalogue revision.
pub trait PipelineRecommendationDefinitionProvider: Send + Sync {
    fn available(&self) -> bool {
        true
    }

    fn definition(
        &self,
        catalogue_revision: &str,
        kind: PipelineKind,
    ) -> Result<Option<PipelineDefinitionSnapshot>>;
}

pub struct UnavailablePipelineRecommendationDefinitions;

impl PipelineRecommendationDefinitionProvider for UnavailablePipelineRecommendationDefinitions {
    fn available(&self) -> bool {
        false
    }

    fn definition(
        &self,
        _catalogue_revision: &str,
        _kind: PipelineKind,
    ) -> Result<Option<PipelineDefinitionSnapshot>> {
        Ok(None)
    }
}

/// The write adapter must lock and recheck the exact current set, snapshot,
/// node, selected Matrix disposition, independent match attestation, source
/// digest, actor/session, and config before capture. Capture inserts the
/// opportunity, capability context, and immutable manifest in one UoW; an
/// exact request replay returns the saved receipt without recapturing.
#[async_trait]
pub trait PipelineRecommendationStore: Send {
    /// Load the immutable captured triple by its exact opportunity ID.
    /// Unconfigured stores deny dispatch rather than accepting caller copies.
    async fn pipeline_recommendation_by_opportunity(
        &mut self,
        _workspace_id: Uuid,
        _opportunity_id: Uuid,
    ) -> Result<Option<PreparedPipelineRecommendation>> {
        Ok(None)
    }

    /// Check the saved triple against current planning, Matrix, source,
    /// catalogue and config state under the adapter's dispatch locks.
    async fn pipeline_recommendation_is_current(
        &mut self,
        _workspace_id: Uuid,
        _saved: &PreparedPipelineRecommendation,
    ) -> Result<bool> {
        Ok(false)
    }

    async fn load_pipeline_recommendation_basis(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        work_node_id: Uuid,
        for_update: bool,
    ) -> Result<Option<PipelineRecommendationBasis>>;

    async fn pipeline_recommendation_by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<PreparedPipelineRecommendation>>;

    async fn capture_pipeline_recommendation(
        &mut self,
        workspace_id: Uuid,
        input: &AdvisoryOpportunityInput,
        context: &PipelineRecommendationContext,
        manifest: &PipelineRecommendationManifest,
    ) -> Result<PreparedPipelineRecommendation>;
}
