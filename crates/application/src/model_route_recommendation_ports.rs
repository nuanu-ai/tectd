use async_trait::async_trait;
use tect_domain::{
    AdvisoryRequestPreference, EligibleModelRoutes, ModelRouteCatalogue, ModelRouteFact,
    ModelRouteRecord, ModelRouteWorkContext, ObservedModelRoute, Result, WorkspaceAdvisoryMode,
};
use uuid::Uuid;

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
    pub work: ModelRouteWorkContext,
    pub advisory_mode: WorkspaceAdvisoryMode,
    pub observed_actual: Option<ObservedModelRoute>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRoutePreparation {
    Prepared,
    WorkspaceDisabled,
    SessionSkip,
    RequestSkip,
    CapabilityUnavailable,
    UnknownWorkFacts,
    NoEligibleRoutes,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedModelRouteRecommendation {
    pub workspace_id: Uuid,
    pub request_key: String,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
    pub work: ModelRouteWorkContext,
    pub catalogue: Option<ModelRouteCatalogue>,
    pub eligible: Option<EligibleModelRoutes>,
    pub preparation: ModelRoutePreparation,
    /// Recommendation stays empty until a separately validated ranking arrives.
    pub routes: ModelRouteRecord,
}

/// Capture must lock and recheck the approved selection, work facts, config,
/// catalogue version/digest, and observed evidence, then persist one immutable
/// receipt atomically. Exact replay returns the original; changed input conflicts.
#[async_trait]
pub trait ModelRouteRecommendationStore: Send {
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
