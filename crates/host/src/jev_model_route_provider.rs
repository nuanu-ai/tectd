//! Explicit native adviser; ranking a route never executes its candidate model.
use crate::system_one_transport::{SystemOneTransport, SystemOneTransportConfig};
use async_trait::async_trait;
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tect_application::{
    ModelRoutePreparedAttempt, ModelRouteProviderObservation, ModelRouteRankingProvider,
    ModelRouteSendPermit, ModelRouteUsage, PreparedModelRouteRecommendation,
};
use tect_domain::{
    AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error, ModelRouteRankingWireOutcome,
    ModelRouteRankingWireRequest, Result,
};
mod wire;
pub const MODEL_ROUTE_CHOICE_WIRE_VERSION: &str = "tect.model-route-typesafe-choice/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevModelRouteConfig {
    pub profile: String,
    pub endpoint: Url,
    pub model: String,
    pub timeout: Duration,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
}

pub struct JevModelRouteProvider {
    config: JevModelRouteConfig,
    binding_digest: String,
    transport: SystemOneTransport,
}

impl JevModelRouteProvider {
    pub fn new(config: JevModelRouteConfig, bearer_credential: String) -> Result<Self> {
        AdvisoryProviderProfileRef {
            id: config.profile.clone(),
        }
        .validate()
        .map_err(|_| Error::InvalidConfiguration)?;
        AdvisoryModelConfiguration {
            model: config.model.clone(),
        }
        .validate()
        .map_err(|_| Error::InvalidConfiguration)?;
        let transport = SystemOneTransport::new(
            SystemOneTransportConfig {
                endpoint: config.endpoint.clone(),
                timeout: config.timeout,
                maximum_request_bytes: config.maximum_request_bytes,
                maximum_response_bytes: config.maximum_response_bytes,
            },
            &bearer_credential,
        )?;
        let binding = serde_json::to_vec(&(
            config.endpoint.as_str(),
            &config.profile,
            &config.model,
            MODEL_ROUTE_CHOICE_WIRE_VERSION,
        ))
        .map_err(|_| Error::InvalidConfiguration)?;
        Ok(Self {
            config,
            binding_digest: format!("{:x}", Sha256::digest(binding)),
            transport,
        })
    }

    fn restore(&self, attempted: &ModelRoutePreparedAttempt) -> Result<()> {
        if attempted.adapter_identity.as_deref() != Some(MODEL_ROUTE_CHOICE_WIRE_VERSION)
            || attempted.request.binding.adviser_model != self.config.model
            || attempted.request_sha256
                != tect_domain::model_route_wire_sha256(&attempted.request_bytes)
            || wire::prepare(
                &attempted.request,
                &self.binding_digest,
                self.config.maximum_request_bytes,
            )? != attempted.request_bytes
        {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    async fn send(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<ModelRouteProviderObservation> {
        self.restore(&attempted)?;
        if permit.attempt_id.is_nil()
            || permit.workspace_id != attempted.request.binding.workspace_id
            || permit.preparation_request_key != attempted.request.binding.preparation_request_key
            || permit.request_sha256 != attempted.request_sha256
            || permit.policy_id.is_nil()
            || permit.policy_version <= 0
            || permit.policy_digest.len() != 64
            || !permit.policy_digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::InputConflict);
        }
        let start = Instant::now();
        let response = self.transport.post_once(&attempted.request_bytes).await?;
        Ok(ModelRouteProviderObservation {
            raw: response.body,
            http_status: Some(response.status),
            input_tokens: None,
            output_tokens: None,
            elapsed_monotonic_ms: i64::try_from(start.elapsed().as_millis()).ok(),
        })
    }
}

#[async_trait]
impl ModelRouteRankingProvider for JevModelRouteProvider {
    fn available(&self) -> bool {
        true
    }
    fn prepare(
        &self,
        saved: &PreparedModelRouteRecommendation,
    ) -> Result<ModelRoutePreparedAttempt> {
        let request = ModelRouteRankingWireRequest::new(
            saved.workspace_id,
            &saved.request_key,
            &saved.work,
            saved.catalogue.as_ref().ok_or(Error::InputConflict)?,
            saved.eligible.as_ref().ok_or(Error::InputConflict)?,
            &self.config.model,
        )?;
        let bytes = wire::prepare(
            &request,
            &self.binding_digest,
            self.config.maximum_request_bytes,
        )?;
        ModelRoutePreparedAttempt::native(request, bytes, MODEL_ROUTE_CHOICE_WIRE_VERSION.into())
    }
    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        Ok(self.send(attempted, permit).await?.raw)
    }
    async fn attempt_prepared_observed(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<ModelRouteProviderObservation> {
        self.send(attempted, permit).await
    }
    fn sealed_usage(
        &self,
        attempted: &ModelRoutePreparedAttempt,
        observation: &ModelRouteProviderObservation,
    ) -> Result<ModelRouteUsage> {
        self.restore(attempted)?;
        wire::usage(&observation.raw, self.config.maximum_response_bytes)
    }
    fn parse_sealed(
        &self,
        attempted: &ModelRoutePreparedAttempt,
        observation: &ModelRouteProviderObservation,
    ) -> Result<ModelRouteRankingWireOutcome> {
        self.restore(attempted)?;
        if !observation
            .http_status
            .is_some_and(|s| (200..300).contains(&s))
        {
            return Err(Error::InvalidArguments);
        }
        wire::parse(
            &attempted.request,
            &observation.raw,
            self.config.maximum_response_bytes,
        )
    }
}

#[cfg(test)]
mod tests;
