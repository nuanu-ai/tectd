//! The Jev adviser behind the application's advice port.
use crate::jev_codec::{decode_answer, encode_request, smallest_answer_bytes};
use crate::jev_transport::{
    JevTransport, MAX_RESPONSE_BYTES, MAX_TIMEOUT_MS, bearer, validate_endpoint,
};
use async_trait::async_trait;
use reqwest::Url;
use std::fmt;
use std::time::Duration;
use tect_application::AdviceProvider;
use tect_domain::{
    AdviceExchange, AdviceFailure, AdviceQuestionSet, AdviceReply, AdviceSendCertainty,
    AdviceUnavailableReason, Error, Result,
};

const MAX_MODEL_BYTES: usize = 128;

/// One client configuration for every advice point.
#[derive(Clone)]
pub struct JevConfig {
    endpoint: Url,
    model: String,
    timeout: Duration,
    api_key: String,
}

impl JevConfig {
    /// Decide from the daemon's `TECT_JEV_ENDPOINT`, `TECT_JEV_MODEL`,
    /// `TECT_JEV_TIMEOUT_MS` and `TYPESAFE_API_KEY`. With none of the three `TECT_JEV_*`
    /// values there is no provider and the key is ignored. With any of them, all three
    /// and the key are required and nothing has a default.
    pub fn from_values(
        endpoint: Option<&str>,
        model: Option<&str>,
        timeout_ms: Option<&str>,
        api_key: Option<&str>,
    ) -> Result<Option<Self>> {
        if endpoint.is_none() && model.is_none() && timeout_ms.is_none() {
            return Ok(None);
        }
        let (Some(endpoint), Some(model), Some(timeout_ms), Some(api_key)) =
            (endpoint, model, timeout_ms, api_key)
        else {
            return Err(Error::InvalidConfiguration);
        };
        let timeout_ms = (!timeout_ms.is_empty() && timeout_ms.bytes().all(|b| b.is_ascii_digit()))
            .then(|| timeout_ms.parse::<u64>().ok())
            .flatten()
            .filter(|value| (1..=MAX_TIMEOUT_MS).contains(value))
            .ok_or(Error::InvalidConfiguration)?;
        if model.is_empty()
            || model.len() > MAX_MODEL_BYTES
            || model.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(Error::InvalidConfiguration);
        }
        bearer(api_key)?;
        Ok(Some(Self {
            endpoint: validate_endpoint(endpoint)?,
            model: model.to_owned(),
            timeout: Duration::from_millis(timeout_ms),
            api_key: api_key.to_owned(),
        }))
    }
}

/// Decide from the daemon's `TECT_JEV_NEW_WORKSPACE_DAILY_LIMIT`. Unset: a workspace
/// the daemon creates has no setting, so Jev is off in it. Set: such a workspace is
/// enabled with this daily call limit, which needs a configured provider; the value
/// is a whole number above zero and nothing stands in for a bad one.
pub fn new_workspace_daily_limit(
    value: Option<&str>,
    provider_configured: bool,
) -> Result<Option<i64>> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !provider_configured {
        return Err(Error::InvalidConfiguration);
    }
    (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
        .then(|| value.parse::<i64>().ok())
        .flatten()
        .filter(|limit| *limit > 0)
        .map(Some)
        .ok_or(Error::InvalidConfiguration)
}

impl fmt::Debug for JevConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JevConfig")
            .field("endpoint", &self.endpoint.as_str())
            .field("model", &self.model)
            .field("timeout", &self.timeout)
            .field("api_key", &"[redacted]")
            .finish()
    }
}

pub struct JevAdviceProvider {
    model: String,
    transport: JevTransport,
}

impl JevAdviceProvider {
    pub fn new(config: JevConfig) -> Result<Self> {
        Ok(Self {
            transport: JevTransport::new(config.endpoint, config.timeout, &config.api_key)?,
            model: config.model,
        })
    }
}

#[async_trait]
impl AdviceProvider for JevAdviceProvider {
    fn configured(&self) -> bool {
        true
    }

    async fn ask(
        &self,
        questions: &AdviceQuestionSet,
    ) -> std::result::Result<AdviceReply, AdviceFailure> {
        // A request whose smallest acceptable answer is over the answer ceiling
        // cannot succeed, so it is not sent.
        let smallest = smallest_answer_bytes(questions);
        if smallest > MAX_RESPONSE_BYTES {
            return Err(AdviceFailure {
                reason: AdviceUnavailableReason::RequestTooLarge,
                send_certainty: AdviceSendCertainty::NotSent,
                detail: format!(
                    "the smallest acceptable answer of {smallest} bytes would exceed \
                     the answer limit of {MAX_RESPONSE_BYTES} bytes"
                ),
                exchange: AdviceExchange {
                    model: Some(self.model.clone()),
                    ..AdviceExchange::default()
                },
            });
        }
        let request = encode_request(&self.model, questions);
        let answer = match self.transport.post_once(&request).await {
            Ok(answer) => answer,
            Err(failure) => {
                let sent = failure.send_certainty != AdviceSendCertainty::NotSent;
                return Err(AdviceFailure {
                    reason: failure.reason,
                    send_certainty: failure.send_certainty,
                    detail: failure.detail,
                    exchange: AdviceExchange {
                        model: Some(self.model.clone()),
                        input_tokens: None,
                        output_tokens: None,
                        latency_ms: failure.latency_ms,
                        request_body: sent.then_some(request),
                        response_body: failure.body,
                    },
                });
            }
        };
        let decoded = decode_answer(&answer.body, questions);
        let exchange = AdviceExchange {
            model: Some(decoded.model.unwrap_or_else(|| self.model.clone())),
            input_tokens: decoded.input_tokens,
            output_tokens: decoded.output_tokens,
            latency_ms: answer.latency_ms,
            request_body: Some(request),
            response_body: Some(answer.body),
        };
        match decoded.answers {
            Ok(answers) => Ok(AdviceReply { answers, exchange }),
            Err(detail) => Err(AdviceFailure {
                reason: AdviceUnavailableReason::InvalidResponse,
                send_certainty: AdviceSendCertainty::Sent,
                detail,
                exchange,
            }),
        }
    }
}

#[cfg(test)]
mod tests;
