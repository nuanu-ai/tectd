use async_trait::async_trait;
use tect_domain::{
    AdvisoryAuditPage, AdvisoryAuditQuery, AdvisoryDispatch, AdvisoryDispatchAuthorization,
    AdvisoryDispatchCancellation, AdvisoryDispatchOutcome, AdvisoryDispatchSeal,
    AdvisoryDispatchStart, AdvisoryModelConfiguration, AdvisoryOpportunity,
    AdvisoryOpportunityDetail, AdvisoryOpportunityInput, AdvisoryProviderProfileRef,
    AdvisoryReconciliationEvidence, AdvisorySendCertainty, ConfigureWorkspaceAdvisory, Result,
    WorkspaceAdvisoryConfig,
};
use uuid::Uuid;

/// Unforgeable outside this crate. Persistence adapters accept it so their
/// public cross-crate trait cannot become a public dispatch-initiation API.
#[doc(hidden)]
pub struct AdvisoryLifecycleCapability(());

impl AdvisoryLifecycleCapability {
    pub(crate) const fn internal() -> Self {
        Self(())
    }
}

/// Slice 0 persistence boundary. The provider is deliberately absent here:
/// every provider send must be authorized and recorded through a later
/// dispatch use case, while no-call opportunities are still durable.
#[async_trait]
pub trait AdvisoryStore: Send {
    async fn advisory_config(&mut self, workspace_id: Uuid) -> Result<WorkspaceAdvisoryConfig>;
    async fn materialize_advisory_config(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
    ) -> Result<WorkspaceAdvisoryConfig>;
    async fn configure_advisory(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &ConfigureWorkspaceAdvisory,
    ) -> Result<WorkspaceAdvisoryConfig>;
    async fn capture_advisory_opportunity(
        &mut self,
        workspace_id: Uuid,
        input: &AdvisoryOpportunityInput,
    ) -> Result<AdvisoryOpportunity>;
    async fn advisory_opportunity_for_dispatch(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunity>;
    async fn advisory_opportunity_by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<AdvisoryOpportunity>>;
    async fn authorize_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        expected_config_revision: i64,
        dispatch: &AdvisoryDispatchAuthorization,
    ) -> Result<AdvisoryDispatch>;
    async fn start_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchStart>;
    async fn seal_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        seal: &AdvisoryDispatchSeal,
    ) -> Result<AdvisoryDispatch>;
    async fn cancel_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchCancellation>;
    async fn reconcile_advisory_dispatch(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        evidence: &AdvisoryReconciliationEvidence,
    ) -> Result<AdvisoryDispatch>;
    async fn finalize_advisory_opportunity(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        expected_config_revision: i64,
        dispatch: &AdvisoryDispatch,
    ) -> Result<AdvisoryOpportunity>;
    async fn advisory_audit(
        &mut self,
        workspace_id: Uuid,
        scope_id: Option<Uuid>,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage>;
    async fn advisory_opportunity_detail(
        &mut self,
        workspace_id: Uuid,
        scope_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunityDetail>;
    async fn advisory_candidate_set_exists(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<bool>;
    async fn candidate_advisory_audit(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage>;
    async fn candidate_advisory_opportunity_detail(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunityDetail>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct AdvisoryProviderRequest {
    pub dispatch_id: Uuid,
    pub provider_profile_ref: AdvisoryProviderProfileRef,
    pub model_configuration: AdvisoryModelConfiguration,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct AdvisoryProviderObservation {
    pub send_certainty: AdvisorySendCertainty,
    pub outcome: AdvisoryDispatchOutcome,
    pub response_payload: Option<Vec<u8>>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub raw_response_ref: Option<String>,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlledAdvisoryDispatch {
    pub dispatch_id: Uuid,
    pub opportunity_id: Uuid,
    pub predecessor_dispatch_id: Option<Uuid>,
    pub attempt_number: i32,
    pub retry_basis: tect_domain::AdvisoryRetryBasis,
    pub payload: Vec<u8>,
}

#[cfg(test)]
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlledAdvisoryResult {
    pub opportunity_id: Uuid,
    pub dispatch: AdvisoryDispatch,
    pub advice_eligible: bool,
}

#[async_trait]
#[allow(dead_code)]
pub(crate) trait AdvisoryProvider: Send + Sync {
    fn identity(&self) -> Option<(&'static str, &'static str)>;
    async fn attempt(
        &self,
        request: &AdvisoryProviderRequest,
    ) -> Result<AdvisoryProviderObservation>;
}

#[derive(Debug, Default)]
pub(crate) struct DisabledAdvisoryProvider;

#[async_trait]
impl AdvisoryProvider for DisabledAdvisoryProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        None
    }

    async fn attempt(&self, _: &AdvisoryProviderRequest) -> Result<AdvisoryProviderObservation> {
        Err(tect_domain::Error::TransportUnavailable)
    }
}
