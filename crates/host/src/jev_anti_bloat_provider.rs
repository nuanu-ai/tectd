//! Explicitly configured native Choice adapter; no environment or default install.
use crate::{
    jev_anti_bloat_choice::{self as choice, CHOICE_WIRE_VERSION, PreparedChoiceRequest},
    system_one_transport::{SystemOneTransport, SystemOneTransportConfig},
};
use async_trait::async_trait;
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use tect_application::{
    AntiBloatProviderObservation, AntiBloatRankingMaterial, AntiBloatRankingOutcome,
    AntiBloatRankingProvider, AntiBloatSendPermit, AntiBloatStartedDispatchPermit, AntiBloatUsage,
};
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef, Error, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevAntiBloatConfig {
    pub profile: String,
    pub endpoint: Url,
    pub model: String,
    pub timeout: Duration,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
}

pub struct JevAntiBloatProvider {
    config: JevAntiBloatConfig,
    provider_binding_digest: String,
    transport: SystemOneTransport,
}

impl JevAntiBloatProvider {
    pub fn new(config: JevAntiBloatConfig, bearer_credential: String) -> Result<Self> {
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
            CHOICE_WIRE_VERSION,
        ))
        .map_err(|_| Error::InvalidConfiguration)?;
        let provider_binding_digest = format!("{:x}", Sha256::digest(binding));
        Ok(Self {
            config,
            provider_binding_digest,
            transport,
        })
    }

    fn restore(&self, permit: &AntiBloatSendPermit) -> Result<PreparedChoiceRequest> {
        choice::restore_choice_request(
            permit,
            &self.config.model,
            CHOICE_WIRE_VERSION,
            &self.provider_binding_digest,
            self.config.maximum_request_bytes,
        )
    }
}

#[async_trait]
impl AntiBloatRankingProvider for JevAntiBloatProvider {
    fn required_profile(&self) -> Option<&str> {
        Some(&self.config.profile)
    }
    fn available(&self) -> bool {
        true
    }
    fn adapter_identity(&self) -> &'static str {
        CHOICE_WIRE_VERSION
    }
    fn prepare(&self, material: &AntiBloatRankingMaterial<'_>) -> Result<Vec<u8>> {
        Ok(choice::prepare_choice_request(
            &self.config.model,
            &self.provider_binding_digest,
            material,
            self.config.maximum_request_bytes,
        )?
        .body()
        .to_vec())
    }

    fn usage_sealed(
        &self,
        permit: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<AntiBloatUsage> {
        self.restore(permit)?;
        choice::decode_choice_usage(&observation.raw, self.config.maximum_response_bytes)
    }

    fn parse_sealed(
        &self,
        permit: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<AntiBloatRankingOutcome> {
        let prepared = self.restore(permit)?;
        if !observation
            .http_status
            .is_some_and(|status| (200..300).contains(&status))
        {
            return Ok(AntiBloatRankingOutcome::InvalidResponse);
        }
        Ok(choice::parse_choice_response(
            &observation.raw,
            &prepared,
            self.config.maximum_response_bytes,
        ))
    }

    async fn rank(
        &self,
        started: &AntiBloatStartedDispatchPermit,
    ) -> Result<AntiBloatProviderObservation> {
        self.restore(started.reservation())?;
        let permit = started.claim()?;
        let start = Instant::now();
        let response = self.transport.post_once(&permit.request.bytes).await?;
        Ok(AntiBloatProviderObservation {
            raw: response.body,
            input_tokens: None,
            output_tokens: None,
            elapsed_monotonic_ms: i64::try_from(start.elapsed().as_millis()).ok(),
            http_status: Some(response.status),
        })
    }
}

#[cfg(test)]
mod tests;
