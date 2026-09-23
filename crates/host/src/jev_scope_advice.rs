use async_trait::async_trait;
use reqwest::{
    Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tect_application::{
    PreparedScopeAdviceAttempt, ScopeAdviceProvider, ScopeAdviceProviderError,
    ScopeAdviceProviderFailureReason, ScopeAdviceProviderObservation, ScopeAdviceProviderRequest,
};
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisorySendCertainty, Error, Result, ScopeAdviceRequest,
};

mod wire;

const WIRE_FORMAT: &str = "jev-system-one-json/1";

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevScopeAdviceConfig {
    pub profile: String,
    pub endpoint: Url,
    pub model: String,
    pub timeout: Duration,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
}

impl JevScopeAdviceConfig {
    fn validate(&self) -> Result<()> {
        if self.profile.is_empty()
            || self.profile.len() > 128
            || self.profile.contains(['\0', ':'])
            || self.model.is_empty()
            || self.model.len() > 128
            || !matches!(self.endpoint.scheme(), "http" | "https")
            || self.endpoint.cannot_be_a_base()
            || !self.endpoint.username().is_empty()
            || self.endpoint.password().is_some()
            || self.endpoint.query().is_some()
            || self.endpoint.fragment().is_some()
            || self.timeout.is_zero()
            || self.maximum_request_bytes == 0
            || self.maximum_response_bytes == 0
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Outer TypeSafe System One adapter. Construction is explicit and does not
/// install the adapter into `WorkspaceService`.
pub struct JevScopeAdviceProvider {
    config: JevScopeAdviceConfig,
    client: reqwest::Client,
    authorization: HeaderValue,
}

impl JevScopeAdviceProvider {
    pub fn new(config: JevScopeAdviceConfig, bearer_credential: String) -> Result<Self> {
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

    pub fn prepare(
        &self,
        request: &ScopeAdviceRequest,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        let body = wire::serialize_request(&self.config.model, request)
            .map_err(|_| ScopeAdviceProviderError::ProvenNotSent)?;
        if body.len() > self.config.maximum_request_bytes {
            return Err(ScopeAdviceProviderError::ProvenNotSent);
        }
        PreparedScopeAdviceAttempt::new(
            request.clone(),
            body,
            self.config.profile.clone(),
            self.config.model.clone(),
            self.config.endpoint.as_str().to_owned(),
            WIRE_FORMAT.to_owned(),
        )
    }

    async fn response_bytes(&self, mut response: reqwest::Response) -> BodyRead {
        let mut bytes = Vec::new();
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(value)) => value,
                Ok(None) => return BodyRead::Complete(bytes),
                Err(_) => return BodyRead::Failed(bytes),
            };
            if bytes.len().saturating_add(chunk.len()) > self.config.maximum_response_bytes {
                let observed_bytes = bytes.len().saturating_add(chunk.len());
                let remaining = self
                    .config
                    .maximum_response_bytes
                    .saturating_sub(bytes.len());
                bytes.extend_from_slice(&chunk[..remaining]);
                return BodyRead::Oversize {
                    partial: bytes,
                    observed_bytes,
                };
            }
            bytes.extend_from_slice(&chunk);
        }
    }

    fn raw_ref(&self, dispatch_id: uuid::Uuid, bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        format!(
            "jev:{}:{dispatch_id}:sha256:{digest:x}",
            self.config.profile
        )
    }

    fn received_failure(
        &self,
        dispatch_id: uuid::Uuid,
        bytes: Vec<u8>,
        observed_bytes: usize,
        reason: ScopeAdviceProviderFailureReason,
        latency_ms: i64,
    ) -> ScopeAdviceProviderObservation {
        let reason_name = match reason {
            ScopeAdviceProviderFailureReason::HttpStatus => "http-status",
            ScopeAdviceProviderFailureReason::InvalidContentType => "content-type",
            ScopeAdviceProviderFailureReason::ResponseOversize => "oversize",
            ScopeAdviceProviderFailureReason::ResponseBodyRead => "body-read",
            ScopeAdviceProviderFailureReason::InvalidResponse => "invalid-response",
        };
        let digest = Sha256::digest(&bytes);
        let raw_response_ref = format!(
            "jev:{}:{dispatch_id}:{reason_name}:observed-{observed_bytes}:retained-{}:sha256:{digest:x}",
            self.config.profile,
            bytes.len()
        );
        ScopeAdviceProviderObservation {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: AdvisoryDispatchOutcome::ProviderFailure,
            answers: None,
            response_payload: Some(bytes),
            input_tokens: None,
            output_tokens: None,
            latency_ms: Some(latency_ms),
            raw_response_ref: Some(raw_response_ref),
            failure_reason: Some(reason),
        }
    }

    pub async fn attempt_prepared(
        &self,
        dispatch_id: uuid::Uuid,
        prepared: PreparedScopeAdviceAttempt,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        if prepared.profile() != self.config.profile
            || prepared.model() != self.config.model
            || prepared.wire_version() != WIRE_FORMAT
            || prepared.destination() != self.config.endpoint.as_str()
            || prepared.body_length() != prepared.body().len()
            || prepared.body_length() > self.config.maximum_request_bytes
            || prepared.body_sha256() != format!("{:x}", Sha256::digest(prepared.body()))
        {
            return Err(ScopeAdviceProviderError::ProvenNotSent);
        }
        let (request, body, _, _, _, _, _, _) = prepared.into_parts();
        let started = std::time::Instant::now();
        let response = self
            .client
            .post(self.config.endpoint.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| ScopeAdviceProviderError::SentUnknown {
                raw_response_ref: Some(format!(
                    "jev:{}:{dispatch_id}:transport-unknown",
                    self.config.profile
                )),
                latency_ms: elapsed_ms(started),
            })?;
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
        let bytes = match self.response_bytes(response).await {
            BodyRead::Complete(value) => value,
            BodyRead::Oversize {
                partial,
                observed_bytes,
            } => {
                return Ok(self.received_failure(
                    dispatch_id,
                    partial,
                    observed_bytes,
                    ScopeAdviceProviderFailureReason::ResponseOversize,
                    elapsed_ms(started),
                ));
            }
            BodyRead::Failed(partial) => {
                let observed_bytes = partial.len();
                return Ok(self.received_failure(
                    dispatch_id,
                    partial,
                    observed_bytes,
                    ScopeAdviceProviderFailureReason::ResponseBodyRead,
                    elapsed_ms(started),
                ));
            }
        };
        let receipt_latency_ms = elapsed_ms(started);
        if !status.is_success() {
            let observed_bytes = bytes.len();
            return Ok(self.received_failure(
                dispatch_id,
                bytes,
                observed_bytes,
                ScopeAdviceProviderFailureReason::HttpStatus,
                receipt_latency_ms,
            ));
        }
        if !is_json {
            let observed_bytes = bytes.len();
            return Ok(self.received_failure(
                dispatch_id,
                bytes,
                observed_bytes,
                ScopeAdviceProviderFailureReason::InvalidContentType,
                receipt_latency_ms,
            ));
        }
        let raw_response_ref = self.raw_ref(dispatch_id, &bytes);
        let Ok(parsed) = wire::parse_response(&bytes, &self.config.model, &request) else {
            let observed_bytes = bytes.len();
            return Ok(self.received_failure(
                dispatch_id,
                bytes,
                observed_bytes,
                ScopeAdviceProviderFailureReason::InvalidResponse,
                receipt_latency_ms,
            ));
        };
        Ok(ScopeAdviceProviderObservation {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: AdvisoryDispatchOutcome::ProviderResponse,
            answers: Some(parsed.answers),
            response_payload: Some(bytes),
            input_tokens: parsed.input_tokens,
            output_tokens: parsed.output_tokens,
            latency_ms: Some(receipt_latency_ms),
            raw_response_ref: Some(raw_response_ref),
            failure_reason: None,
        })
    }

    #[cfg(test)]
    async fn attempt_request(
        &self,
        dispatch_id: uuid::Uuid,
        request: &ScopeAdviceRequest,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        let prepared = self.prepare(request)?;
        self.attempt_prepared(dispatch_id, prepared).await
    }
}

enum BodyRead {
    Complete(Vec<u8>),
    Oversize {
        partial: Vec<u8>,
        observed_bytes: usize,
    },
    Failed(Vec<u8>),
}

fn elapsed_ms(started: std::time::Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

#[async_trait]
impl ScopeAdviceProvider for JevScopeAdviceProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("jev-system-one", "1"))
    }

    fn prepare(
        &self,
        request: &ScopeAdviceRequest,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        JevScopeAdviceProvider::prepare(self, request)
    }

    async fn attempt_prepared(
        &self,
        request: &ScopeAdviceProviderRequest,
        prepared: PreparedScopeAdviceAttempt,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        if prepared.request() != &request.request {
            return Err(ScopeAdviceProviderError::ProvenNotSent);
        }
        JevScopeAdviceProvider::attempt_prepared(self, request.dispatch_id, prepared).await
    }
}
