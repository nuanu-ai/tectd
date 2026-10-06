//! Private single-request transport. Callers own the durable dispatch fence.
use reqwest::{
    Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use sha2::{Digest, Sha256};
use std::{net::IpAddr, time::Duration};
use tect_application::AdvisoryProviderTransportContext;
use tect_domain::{AdvisoryDispatchOutcome, AdvisorySendCertainty};
use tect_domain::{Error, Result};

#[derive(Clone)]
pub(crate) struct SystemOneTransportConfig {
    pub endpoint: Url,
    pub timeout: Duration,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
}

impl SystemOneTransportConfig {
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
        if !(self.endpoint.scheme() == "https"
            || (self.endpoint.scheme() == "http" && numeric_loopback))
            || self.endpoint.cannot_be_a_base()
            || self.endpoint.host_str().is_none()
            || !self.endpoint.username().is_empty()
            || self.endpoint.password().is_some()
            || self.endpoint.query().is_some()
            || self.endpoint.fragment().is_some()
            || self.endpoint.path() != "/v1/systemone"
            || self.timeout.is_zero()
            || self.maximum_request_bytes == 0
            || self.maximum_response_bytes == 0
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
}

pub(crate) struct RawSystemOneResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub response_complete: bool,
    pub original_transport_context: AdvisoryProviderTransportContext,
}

impl RawSystemOneResponse {
    fn captured(
        status: u16,
        body: Vec<u8>,
        response_complete: bool,
        failure: Option<&str>,
    ) -> Self {
        let failed = !response_complete || !(200..300).contains(&status);
        let original_transport_context = AdvisoryProviderTransportContext {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: if failed {
                AdvisoryDispatchOutcome::ProviderFailure
            } else {
                AdvisoryDispatchOutcome::ProviderResponse
            },
            raw_response_ref: Some(format!("sha256:{:x}", Sha256::digest(&body))),
            provider_failure_code: failure
                .map(str::to_owned)
                .or_else(|| failed.then(|| "http-status".into())),
        };
        Self {
            status,
            body,
            response_complete,
            original_transport_context,
        }
    }
}

pub(crate) struct SystemOneTransport {
    config: SystemOneTransportConfig,
    client: reqwest::Client,
    authorization: HeaderValue,
}

impl SystemOneTransport {
    pub(crate) fn new(config: SystemOneTransportConfig, bearer_credential: &str) -> Result<Self> {
        config.validate()?;
        if bearer_credential.trim().is_empty() {
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

    /// One POST of frozen bytes. Once headers arrive, retain bounded raw evidence.
    pub(crate) async fn post_once(&self, exact_body: &[u8]) -> Result<RawSystemOneResponse> {
        if exact_body.is_empty() || exact_body.len() > self.config.maximum_request_bytes {
            return Err(Error::RequestTooLarge);
        }
        let mut response = self
            .client
            .post(self.config.endpoint.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(exact_body.to_vec())
            .send()
            .await
            .map_err(|_| Error::TransportUnavailable)?;
        let status = response.status().as_u16();
        let mut body = Vec::new();
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => return Ok(RawSystemOneResponse::captured(status, body, true, None)),
                Err(_) => {
                    return Ok(RawSystemOneResponse::captured(
                        status,
                        body,
                        false,
                        Some("response-body-read"),
                    ));
                }
            };
            if body.len().saturating_add(chunk.len()) > self.config.maximum_response_bytes {
                let retained = self.config.maximum_response_bytes - body.len();
                body.extend_from_slice(&chunk[..retained]);
                return Ok(RawSystemOneResponse::captured(
                    status,
                    body,
                    false,
                    Some("response-oversize"),
                ));
            }
            body.extend_from_slice(&chunk);
        }
    }
}

#[cfg(test)]
mod tests;
