use super::WorkspaceService;
use tect_domain::Result;

impl WorkspaceService {
    pub(crate) async fn seal_committed_model_route_response(
        &self,
        tenant_id: uuid::Uuid,
        permit: &crate::ModelRouteSendPermit,
        raw: &[u8],
    ) -> Result<()> {
        self.store
            .seal_committed_model_route_response(tenant_id, permit, raw)
            .await
    }

    pub(crate) async fn consume_committed_model_route_budget(
        &self,
        tenant_id: uuid::Uuid,
        permit: &crate::ModelRouteSendPermit,
        observation: &crate::ModelRouteProviderObservation,
    ) -> Result<bool> {
        self.store
            .consume_committed_model_route_budget(tenant_id, permit, observation)
            .await
    }

    pub(crate) async fn record_committed_model_route_failure(
        &self,
        tenant_id: uuid::Uuid,
        permit: &crate::ModelRouteSendPermit,
    ) -> Result<()> {
        self.store
            .record_committed_model_route_failure(tenant_id, permit)
            .await
    }

    pub(crate) fn model_route_advisory_inputs(
        &self,
    ) -> (
        &dyn crate::ModelRouteHostCapabilitiesProvider,
        &dyn crate::ModelRouteCatalogueProvider,
    ) {
        (
            &*self.model_route_host_capabilities_provider,
            &*self.model_route_catalogue_provider,
        )
    }
}
