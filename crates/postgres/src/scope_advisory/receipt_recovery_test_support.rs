use crate::PgStore;
use std::sync::atomic::{AtomicBool, Ordering};
use tect_application::{
    AdvisoryDispatchContinuation, AdvisoryProviderReceiptObservation, AdvisoryProviderReceiptUsage,
    Store, StoredAdvisoryProviderReceipt, TransactionMode, UnitOfWork,
};
use tect_domain::{AdvisoryBudgetConsumption, Error, Result};

/// Inject only a process interruption after production raw commit. No row/state fabrication.
pub(super) struct InterruptAfterRaw {
    pub(super) inner: PgStore,
    pub(super) pending: AtomicBool,
}

#[async_trait::async_trait]
impl Store for InterruptAfterRaw {
    async fn begin(&self, mode: TransactionMode) -> Result<Box<dyn UnitOfWork>> {
        self.inner.begin(mode).await
    }
    async fn seal_committed_advisory_observation(
        &self,
        continuation: &AdvisoryDispatchContinuation,
        observation: &AdvisoryProviderReceiptObservation,
        elapsed: i64,
    ) -> Result<StoredAdvisoryProviderReceipt> {
        self.inner
            .seal_committed_advisory_observation(continuation, observation, elapsed)
            .await
    }
    async fn consume_committed_advisory_observation(
        &self,
        continuation: &AdvisoryDispatchContinuation,
        usage: AdvisoryProviderReceiptUsage,
    ) -> Result<(StoredAdvisoryProviderReceipt, AdvisoryBudgetConsumption)> {
        if self.pending.swap(false, Ordering::SeqCst) {
            return Err(Error::StorageUnavailable);
        }
        self.inner
            .consume_committed_advisory_observation(continuation, usage)
            .await
    }
}
