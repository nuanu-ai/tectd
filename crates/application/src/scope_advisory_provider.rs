use crate::{
    PreparedScopeAdviceAttempt, ScopeAdviceProviderContext, ScopeAdviceProviderError,
    ScopeAdviceProviderObservation, ScopeAdviceProviderRequest,
};
use async_trait::async_trait;
use tect_domain::{NormalizedScopeAdviceAnswers, Result};

#[async_trait]
pub trait ScopeAdviceProvider: Send + Sync {
    fn identity(&self) -> Option<(&'static str, &'static str)>;
    /// Pure, no-I/O serialization and provider-target binding. Rejections are
    /// proven not sent and must be recorded as a no-call before authorization.
    fn prepare_context(
        &self,
        context: &ScopeAdviceProviderContext,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError>;
    /// Accept a sealed attempt only when its exact body still represents the
    /// current authorized context. Providers may recognize an older wire
    /// format here without changing the format used for new sends.
    fn prepared_matches_context(
        &self,
        context: &ScopeAdviceProviderContext,
        prepared: &PreparedScopeAdviceAttempt,
    ) -> bool {
        self.prepare_context(context)
            .is_ok_and(|current| current == *prepared)
    }
    /// `Ok` is reserved for a transport result proven sent, including typed
    /// provider/body failures. Pre-response uncertainty uses the error variant.
    async fn attempt_prepared(
        &self,
        request: &ScopeAdviceProviderRequest,
        prepared: PreparedScopeAdviceAttempt,
        permit: crate::scope_advisory_orchestration::StartedScopeDispatchPermit,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError>;
    /// Compatibility bridge only. Native implementations return no typed answers.
    async fn observe_prepared(
        &self,
        request: &ScopeAdviceProviderRequest,
        prepared: PreparedScopeAdviceAttempt,
        permit: crate::scope_advisory_orchestration::StartedScopeDispatchPermit,
    ) -> std::result::Result<crate::ScopeAdviceRawObservation, ScopeAdviceProviderError> {
        self.attempt_prepared(request, prepared, permit)
            .await
            .map(crate::ScopeAdviceRawObservation::from_legacy)
    }
    /// Pure accounting from already durable, complete evidence.
    fn usage_from_sealed_response(
        &self,
        saved: &crate::StoredAdvisoryProviderReceipt,
    ) -> Result<crate::AdvisoryProviderReceiptUsage> {
        Ok(crate::scope_advisory_provider_receipt::original_usage(
            saved,
        ))
    }
    /// Pure native interpretation. Legacy answers exist only in the fresh bridge.
    fn parse_sealed_response(
        &self,
        _prepared: &PreparedScopeAdviceAttempt,
        _saved: &crate::StoredAdvisoryProviderReceipt,
    ) -> Result<NormalizedScopeAdviceAnswers> {
        Err(tect_domain::Error::TransportUnavailable)
    }
}
