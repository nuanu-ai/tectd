use super::WorkspaceService;
use tect_domain::Result;

impl WorkspaceService {
    pub(crate) async fn seal_committed_advisory_observation(
        &self,
        continuation: &crate::AdvisoryDispatchContinuation,
        observation: &crate::AdvisoryProviderReceiptObservation,
        elapsed: i64,
    ) -> Result<crate::StoredAdvisoryProviderReceipt> {
        if continuation.tenant_id().is_nil() {
            return Err(tect_domain::Error::InputConflict);
        }
        self.store
            .seal_committed_advisory_observation(continuation, observation, elapsed)
            .await
    }

    pub(crate) async fn consume_committed_advisory_observation(
        &self,
        continuation: &crate::AdvisoryDispatchContinuation,
        usage: crate::AdvisoryProviderReceiptUsage,
    ) -> Result<(
        crate::StoredAdvisoryProviderReceipt,
        tect_domain::AdvisoryBudgetConsumption,
    )> {
        self.store
            .consume_committed_advisory_observation(continuation, usage)
            .await
    }

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
}
