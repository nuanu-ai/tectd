//! Explicit TypeSafe Pipeline HTTP transport. Never installed by default.

use std::{net::IpAddr, time::Duration};

use async_trait::async_trait;
use reqwest::{
    Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tect_application::{
    AdvisoryProviderReceiptObservation, AdvisoryProviderReceiptUsage, PipelineProviderIdentity,
    PipelineProviderObservation, PipelineRecommendationProvider, PipelineStartedDispatchPermit,
    PreparedPipelineRecommendation, PreparedPipelineRecommendationAttempt,
    SealedPipelineRecommendationResponse, StoredAdvisoryProviderReceipt,
};

mod receipt;
use tect_domain::{Error, PipelineRecommendationManifest, PipelineRecommendationRanking, Result};

use super::{
    ENDPOINT_PATH, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, WIRE_VERSION,
    parse_sealed_native_response, prepare_native_request,
};

/// Explicit transport configuration. HTTP is accepted only for numeric loopback stubs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevPipelineConfig {
    pub identity: PipelineProviderIdentity,
    pub endpoint: Url,
    pub timeout: Duration,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
}

impl JevPipelineConfig {
    fn validate(&self) -> Result<()> {
        let numeric_loopback = self
            .endpoint
            .host_str()
            .and_then(|host| {
                host.trim_start_matches('[')
                    .trim_end_matches(']')
                    .parse::<IpAddr>()
                    .ok()
            })
            .is_some_and(|address| address.is_loopback());
        if self.identity.provider.is_empty()
            || self.identity.model.is_empty()
            || self.identity.model.len() > 128
            || self.identity.model.chars().any(char::is_control)
            || self.identity.wire_version != WIRE_VERSION
            || self.identity.destination != self.endpoint.as_str()
            || self.identity.destination.len() > 256
            || !(self.endpoint.scheme() == "https"
                || (self.endpoint.scheme() == "http" && numeric_loopback))
            || self.endpoint.cannot_be_a_base()
            || !self.endpoint.username().is_empty()
            || self.endpoint.password().is_some()
            || self.endpoint.query().is_some()
            || self.endpoint.fragment().is_some()
            || self.endpoint.path() != ENDPOINT_PATH
            || self.timeout.is_zero()
            || self.maximum_request_bytes == 0
            || self.maximum_request_bytes > MAX_REQUEST_BYTES
            || self.maximum_response_bytes == 0
            || self.maximum_response_bytes > MAX_RESPONSE_BYTES
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Constructed only by an explicit caller; the default pipeline provider stays disabled.
pub struct JevPipelineProvider {
    config: JevPipelineConfig,
    client: reqwest::Client,
    authorization: HeaderValue,
}

impl JevPipelineProvider {
    pub fn new(config: JevPipelineConfig, bearer_credential: String) -> Result<Self> {
        config.validate()?;
        if bearer_credential.is_empty() {
            return Err(Error::InvalidConfiguration);
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {bearer_credential}"))
            .map_err(|_| Error::InvalidConfiguration)?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build()
            .map_err(|_| Error::InvalidConfiguration)?;
        Ok(Self {
            config,
            client,
            authorization,
        })
    }

    pub(super) async fn send_once(&self, body: Vec<u8>) -> Result<PipelineProviderObservation> {
        let response = self
            .client
            .post(self.config.endpoint.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| Error::TransportUnavailable)?;
        let status = response.status();
        let is_json = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
            });
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::TransportUnavailable)?
        {
            if bytes.len().saturating_add(chunk.len()) > self.config.maximum_response_bytes {
                return Err(Error::RequestTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() || !is_json || bytes.is_empty() {
            return Err(Error::TransportUnavailable);
        }
        // Usage is advisory accounting data. Invalid or absent fields stay unknown;
        // ranking validation happens only after raw bytes are durably sealed.
        let usage = crate::jev_json::decode_unique_json(&bytes)
            .ok()
            .and_then(|value| value.get("usage").cloned());
        let tokens = usage.as_ref().and_then(Value::as_object);
        Ok(PipelineProviderObservation {
            raw_response: bytes,
            input_tokens: tokens
                .and_then(|value| value.get("input_tokens"))
                .and_then(Value::as_u64),
            output_tokens: tokens
                .and_then(|value| value.get("output_tokens"))
                .and_then(Value::as_u64),
        })
    }
}

#[async_trait]
impl PipelineRecommendationProvider for JevPipelineProvider {
    async fn observe_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<AdvisoryProviderReceiptObservation> {
        let dispatch_id = permit.dispatch_id();
        if !permit.permits(&prepared)
            || prepared.identity() != &self.config.identity
            || prepared.body().len() > self.config.maximum_request_bytes
            || prepared.body_sha256() != format!("{:x}", Sha256::digest(prepared.body()))
        {
            return Err(Error::InputConflict);
        }
        receipt::validate_body(self, prepared.body(), prepared.manifest_digest())?;
        self.observe_once(dispatch_id, prepared.body().to_vec())
            .await
    }

    fn usage_from_sealed_response(
        &self,
        saved: &StoredAdvisoryProviderReceipt,
    ) -> Result<AdvisoryProviderReceiptUsage> {
        receipt::usage(self, saved)
    }

    fn prepare(
        &self,
        saved: &PreparedPipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendationAttempt> {
        let wire = prepare_native_request(
            &self.config.identity.model,
            &saved.manifest,
            self.config.maximum_request_bytes,
        )?;
        PreparedPipelineRecommendationAttempt::new(saved, self.config.identity.clone(), wire.body)
    }

    fn parse_sealed_response(
        &self,
        manifest: &PipelineRecommendationManifest,
        attempted: &PreparedPipelineRecommendationAttempt,
        sealed: &SealedPipelineRecommendationResponse,
    ) -> Result<PipelineRecommendationRanking> {
        if attempted.identity() != &self.config.identity
            || sealed.bytes().len() > self.config.maximum_response_bytes
        {
            return Err(Error::InputConflict);
        }
        Ok(parse_sealed_native_response(manifest, attempted, sealed)?.ranking)
    }

    async fn attempt_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<PipelineProviderObservation> {
        if !permit.permits(&prepared)
            || prepared.identity() != &self.config.identity
            || prepared.body().len() > self.config.maximum_request_bytes
            || prepared.body_sha256() != format!("{:x}", Sha256::digest(prepared.body()))
        {
            return Err(Error::InputConflict);
        }
        self.send_once(prepared.body().to_vec()).await
    }
}
