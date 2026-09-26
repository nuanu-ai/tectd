//! Narrow durable boundary for one pipeline recommendation provider attempt.

use crate::PipelineProviderObservation;
use async_trait::async_trait;
use tect_domain::{AdvisoryDispatch, AdvisoryDispatchAuthorization, AdvisoryDispatchStart, Result};
use uuid::Uuid;

/// Only the application use case can initiate a pipeline dispatch through a
/// cross-crate store trait. A prepared recommendation alone carries no grant.
#[doc(hidden)]
pub struct PipelineDispatchCapability(());

impl PipelineDispatchCapability {
    pub(crate) const fn internal() -> Self {
        Self(())
    }
}

pub struct StoredPipelineRecommendationDispatch {
    pub dispatch: AdvisoryDispatch,
    pub request_payload: Vec<u8>,
    pub response_payload: Vec<u8>,
    pub response_sha256: String,
}

#[async_trait]
pub trait PipelineRecommendationDispatchStore: Send {
    /// Historical typed raw fallback only when no normalized interpretation exists.
    async fn pipeline_dispatch_for_replay(
        &mut self,
        _workspace_id: Uuid,
        _opportunity_id: Uuid,
    ) -> Result<Option<StoredPipelineRecommendationDispatch>> {
        Err(tect_domain::Error::Forbidden)
    }
    /// Commit the UoW before using a `PipelineStartedDispatchPermit`. Replaying
    /// an authorized dispatch may read its identity, but cannot start it twice.
    async fn authorize_pipeline_dispatch(
        &mut self,
        capability: &PipelineDispatchCapability,
        workspace_id: Uuid,
        expected_config_revision: i64,
        authorization: &AdvisoryDispatchAuthorization,
    ) -> Result<AdvisoryDispatch>;

    async fn start_pipeline_dispatch(
        &mut self,
        capability: &PipelineDispatchCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchStart>;

    /// Persists exactly the raw response bytes and SHA-256 in one transaction.
    /// An exact sealed replay is read-only; a changed response is rejected.
    async fn seal_pipeline_dispatch(
        &mut self,
        capability: &PipelineDispatchCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
        observation: &PipelineProviderObservation,
        elapsed_ms: i64,
    ) -> Result<StoredPipelineRecommendationDispatch>;
}
