//! One POST to Jev: no retry, no redirect, no proxy, bounded in both directions.
use reqwest::{
    Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use std::time::{Duration, Instant};
use tect_domain::{AdviceSendCertainty, AdviceUnavailableReason, Error, Result};

/// Ceiling of one request body sent to Jev.
pub(crate) const MAX_REQUEST_BYTES: usize = 256 * 1024;
/// Ceiling of one answer body read from Jev.
pub(crate) const MAX_RESPONSE_BYTES: usize = 64 * 1024;
/// Upper bound of `TECT_JEV_TIMEOUT_MS`; below the daemon's 45 s operation deadline.
pub(crate) const MAX_TIMEOUT_MS: u64 = 30_000;
const PRODUCTION_ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const BEARER: &str = "Bearer ";

/// Accept the production endpoint, or the same path on `http://127.0.0.1:<port>` for
/// tests. Nothing else may receive the key.
pub(crate) fn validate_endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value).map_err(|_| Error::InvalidConfiguration)?;
    let loopback = url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url
            .port()
            .is_some_and(|port| url.as_str() == format!("http://127.0.0.1:{port}/v1/systemone"));
    if url.as_str() == PRODUCTION_ENDPOINT || loopback {
        Ok(url)
    } else {
        Err(Error::InvalidConfiguration)
    }
}

/// The authorization header of a usable key; its value is never printed by `Debug`.
pub(crate) fn bearer(api_key: &str) -> Result<HeaderValue> {
    if api_key.trim().is_empty() {
        return Err(Error::InvalidConfiguration);
    }
    let mut authorization = HeaderValue::from_str(&format!("{BEARER}{api_key}"))
        .map_err(|_| Error::InvalidConfiguration)?;
    authorization.set_sensitive(true);
    Ok(authorization)
}

/// A complete answer with a success status.
pub(crate) struct JevAnswer {
    pub body: Vec<u8>,
    pub latency_ms: Option<i64>,
}

/// Why there is no usable answer, with whatever was received.
pub(crate) struct JevTransportFailure {
    pub reason: AdviceUnavailableReason,
    pub send_certainty: AdviceSendCertainty,
    pub detail: String,
    pub body: Option<Vec<u8>>,
    pub latency_ms: Option<i64>,
}

pub(crate) struct JevTransport {
    endpoint: Url,
    timeout: Duration,
    client: reqwest::Client,
    authorization: HeaderValue,
    request_ceiling: usize,
    response_ceiling: usize,
}

impl JevTransport {
    pub(crate) fn new(endpoint: Url, timeout: Duration, api_key: &str) -> Result<Self> {
        if timeout.is_zero() {
            return Err(Error::InvalidConfiguration);
        }
        let authorization = bearer(api_key)?;
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            // A kept connection that died would be re-sent on a new one by the client.
            .pool_max_idle_per_host(0)
            .no_proxy()
            .build()
            .map_err(|_| Error::InvalidConfiguration)?;
        Ok(Self {
            endpoint,
            timeout,
            client,
            authorization,
            request_ceiling: MAX_REQUEST_BYTES,
            response_ceiling: MAX_RESPONSE_BYTES,
        })
    }

    /// Send the exact bytes once and read at most the response ceiling.
    pub(crate) async fn post_once(
        &self,
        body: &[u8],
    ) -> std::result::Result<JevAnswer, JevTransportFailure> {
        if body.len() > self.request_ceiling {
            return Err(JevTransportFailure {
                reason: AdviceUnavailableReason::RequestTooLarge,
                send_certainty: AdviceSendCertainty::NotSent,
                detail: format!(
                    "the request of {} bytes exceeds the limit of {} bytes",
                    body.len(),
                    self.request_ceiling
                ),
                body: None,
                latency_ms: None,
            });
        }
        let started = Instant::now();
        let elapsed = || i64::try_from(started.elapsed().as_millis()).ok();
        let sent = self
            .client
            .post(self.endpoint.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body.to_vec())
            .send()
            .await;
        let mut response = match sent {
            Ok(response) => response,
            Err(error) if error.is_connect() => {
                return Err(JevTransportFailure {
                    reason: AdviceUnavailableReason::TransportError,
                    send_certainty: AdviceSendCertainty::NotSent,
                    detail: "no connection to Jev could be made".into(),
                    body: None,
                    latency_ms: None,
                });
            }
            Err(error) => {
                let (reason, detail) = if error.is_timeout() {
                    (AdviceUnavailableReason::Timeout, self.timed_out())
                } else {
                    (
                        AdviceUnavailableReason::TransportError,
                        "the request to Jev failed before an answer arrived".into(),
                    )
                };
                return Err(JevTransportFailure {
                    reason,
                    send_certainty: AdviceSendCertainty::SentUnknown,
                    detail,
                    body: None,
                    latency_ms: elapsed(),
                });
            }
        };

        let status = response.status();
        let mut received = Vec::new();
        let mut broken = None;
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    let room = self.response_ceiling - received.len();
                    if chunk.len() > room {
                        received.extend_from_slice(&chunk[..room]);
                        broken = Some((
                            AdviceUnavailableReason::InvalidResponse,
                            format!(
                                "the answer exceeds the limit of {} bytes",
                                self.response_ceiling
                            ),
                        ));
                        break;
                    }
                    received.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(error) => {
                    broken = Some(if error.is_timeout() {
                        (AdviceUnavailableReason::Timeout, self.timed_out())
                    } else {
                        (
                            AdviceUnavailableReason::TransportError,
                            "the answer from Jev broke off".into(),
                        )
                    });
                    break;
                }
            }
        }
        let body = self.without_key(received);
        let failure = if status.is_success() {
            broken
        } else {
            Some((
                AdviceUnavailableReason::HttpStatus,
                format!("Jev answered with HTTP status {}", status.as_u16()),
            ))
        };
        match failure {
            None => Ok(JevAnswer {
                body,
                latency_ms: elapsed(),
            }),
            Some((reason, detail)) => Err(JevTransportFailure {
                reason,
                send_certainty: AdviceSendCertainty::Sent,
                detail,
                body: Some(body),
                latency_ms: elapsed(),
            }),
        }
    }

    fn timed_out(&self) -> String {
        format!("Jev did not answer within {} ms", self.timeout.as_millis())
    }

    /// An answer that echoes the key must not carry it into the journal.
    fn without_key(&self, body: Vec<u8>) -> Vec<u8> {
        let key = &self.authorization.as_bytes()[BEARER.len()..];
        let mut clean = Vec::with_capacity(body.len());
        let mut rest = body.as_slice();
        while let Some(at) = rest.windows(key.len()).position(|window| window == key) {
            clean.extend_from_slice(&rest[..at]);
            clean.extend_from_slice(b"[redacted]");
            rest = &rest[at + key.len()..];
        }
        clean.extend_from_slice(rest);
        clean
    }
}

#[cfg(test)]
pub(crate) mod tests;
