use async_trait::async_trait;
use tect_domain::{
    ModelRouteCatalogue, ModelRouteFact, ModelRouteWorkContext, Result, WorkspaceAdvisoryMode,
};
use uuid::Uuid;

pub use tect_domain::{
    CapturedModelRouteDecision, CapturedModelRouteDisposition, ModelRouteAbstainReason,
    ModelRouteDecisionInput, ModelRouteDecisionOutcome, ModelRouteDispositionAction,
    ModelRoutePreparation, PreparedModelRouteRecommendation,
};

/// A host-owned, immutable snapshot. None means the capability is unavailable.
pub trait ModelRouteCatalogueProvider: Send + Sync {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>>;
}

pub struct UnavailableModelRouteCatalogue;

impl ModelRouteCatalogueProvider for UnavailableModelRouteCatalogue {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>> {
        Ok(None)
    }
}

/// Host-owned discovery, independent of caller Work and the allowed-route catalogue.
pub trait ModelRouteHostCapabilitiesProvider: Send + Sync {
    fn host_capabilities(&self) -> Result<ModelRouteFact<Vec<String>>>;
}

pub struct UnavailableModelRouteHostCapabilities;

impl ModelRouteHostCapabilitiesProvider for UnavailableModelRouteHostCapabilities {
    fn host_capabilities(&self) -> Result<ModelRouteFact<Vec<String>>> {
        Ok(ModelRouteFact::Unknown)
    }
}

/// The adapter reads the persisted, approved Slice 02 selection and independent
/// execution evidence. Caller-supplied Matrix fields must never replace it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteRecommendationBasis {
    pub advisory_mode: WorkspaceAdvisoryMode,
    pub advisory_config_revision: i64,
}

/// Capture must lock and recheck the approved selection, work facts, config,
/// catalogue version/digest, and observed evidence, then persist one immutable
/// receipt atomically. Exact replay returns the original; changed input conflicts.
#[async_trait]
pub trait ModelRouteRecommendationStore: Send {
    async fn validate_current(
        &mut self,
        _prepared: &PreparedModelRouteRecommendation,
    ) -> Result<()> {
        Err(tect_domain::Error::Forbidden)
    }
    async fn by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<PreparedModelRouteRecommendation>>;

    async fn load_basis(
        &mut self,
        workspace_id: Uuid,
        disposition_id: Uuid,
    ) -> Result<Option<ModelRouteRecommendationBasis>>;

    async fn capture(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
    ) -> Result<PreparedModelRouteRecommendation>;
}

/// Read-only provenance boundary for one exact, current Matrix-selected native save.
/// Missing typed work facts remain Unknown; this port never dispatches a model.
#[async_trait]
pub trait ModelRouteSelectionRead: Send {
    async fn approved_work_context(
        &mut self,
        workspace_id: Uuid,
        disposition_id: Uuid,
        candidate_set_id: Uuid,
        caller_request_id: Uuid,
        mapped_work_node_id: Uuid,
        mapped_work_node_revision: i64,
    ) -> Result<Option<ModelRouteWorkContext>>;
}

/// Implementations must lock/recheck the immutable preparation and exact
/// Matrix/Work/catalogue basis before insert. Replay is exact; conflicts fail.
#[async_trait]
pub trait ModelRouteDecisionStore: Send {
    /// Only a sealed raw provider response for the exact saved preparation may
    /// authorize a Ranking. Default deny keeps old adapters from laundering
    /// caller-supplied IDs as Jev advice.
    async fn sealed_provider_ranking(
        &mut self,
        _workspace_id: Uuid,
        _preparation_request_key: &str,
    ) -> Result<Option<crate::ModelRouteSealedRankingEvidence>> {
        Ok(None)
    }
    async fn decision_by_id(
        &mut self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CapturedModelRouteDecision>>;
    async fn decision_by_preparation(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<CapturedModelRouteDecision>>;
    async fn capture_decision(
        &mut self,
        value: &CapturedModelRouteDecision,
    ) -> Result<CapturedModelRouteDecision>;
    async fn disposition_by_id(
        &mut self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CapturedModelRouteDisposition>>;
    async fn disposition_by_decision(
        &mut self,
        workspace_id: Uuid,
        decision_id: Uuid,
    ) -> Result<Option<CapturedModelRouteDisposition>>;
    async fn capture_disposition(
        &mut self,
        value: &CapturedModelRouteDisposition,
    ) -> Result<CapturedModelRouteDisposition>;
}
