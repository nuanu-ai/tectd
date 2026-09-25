use super::{
    PipelineProviderObservation, PipelineRecommendationProvider, PipelineStartedDispatchPermit,
    PreparedPipelineRecommendationAttempt, SealedPipelineRecommendationResponse,
};
use crate::PreparedPipelineRecommendation;
use async_trait::async_trait;
use tect_domain::{Error, PipelineRecommendationManifest, PipelineRecommendationRanking, Result};

pub struct DisabledPipelineRecommendationProvider;

#[async_trait]
impl PipelineRecommendationProvider for DisabledPipelineRecommendationProvider {
    fn available(&self) -> bool {
        false
    }

    fn prepare(
        &self,
        _: &PreparedPipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendationAttempt> {
        Err(Error::TransportUnavailable)
    }

    fn parse_sealed_response(
        &self,
        _: &PipelineRecommendationManifest,
        _: &PreparedPipelineRecommendationAttempt,
        _: &SealedPipelineRecommendationResponse,
    ) -> Result<PipelineRecommendationRanking> {
        Err(Error::TransportUnavailable)
    }

    async fn attempt_prepared(
        &self,
        _: PreparedPipelineRecommendationAttempt,
        _: PipelineStartedDispatchPermit,
    ) -> Result<PipelineProviderObservation> {
        Err(Error::TransportUnavailable)
    }
}
