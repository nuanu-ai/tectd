use super::WorkspaceService;
use tect_domain::Result;

impl WorkspaceService {
    pub(crate) async fn seal_committed_matrix_observation(
        &self,
        tenant: uuid::Uuid,
        continuation: &crate::MatrixDispatchContinuation,
        observation: &crate::MatrixProviderObservation,
        elapsed: i64,
    ) -> Result<crate::StoredMatrixDispatch> {
        self.store
            .seal_committed_matrix_observation(tenant, continuation, observation, elapsed)
            .await
    }
    pub(crate) async fn consume_committed_matrix_observation(
        &self,
        tenant: uuid::Uuid,
        continuation: &crate::MatrixDispatchContinuation,
        usage: crate::MatrixProviderUsage,
    ) -> Result<(
        crate::StoredMatrixDispatch,
        tect_domain::AdvisoryBudgetConsumption,
    )> {
        self.store
            .consume_committed_matrix_observation(tenant, continuation, usage)
            .await
    }
    pub(crate) async fn seal_committed_model_route_observation(
        &self,
        tenant_id: uuid::Uuid,
        permit: &crate::ModelRouteSendPermit,
        observation: &crate::ModelRouteProviderObservation,
    ) -> Result<()> {
        self.store
            .seal_committed_model_route_observation(tenant_id, permit, observation)
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
