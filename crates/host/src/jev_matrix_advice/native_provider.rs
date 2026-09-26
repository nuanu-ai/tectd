//! Explicitly constructed native TypeSafe Matrix adapter. Nothing in the host
//! installs it by default; the default Matrix provider and budget deny remain.

use std::{collections::BTreeMap, net::IpAddr, time::Duration};

use async_trait::async_trait;
use reqwest::{
    Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_application::{
    MAX_PREPARED_MATRIX_BODY_BYTES, MatrixAdviceProvider, MatrixProviderIdentity,
    MatrixProviderObservation, MatrixProviderRequest, MatrixProviderResponse, MatrixProviderUsage,
    MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt, StoredMatrixDispatch,
};
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisoryDispatchState, AdvisorySendCertainty, Error,
    MatrixAdviceEligibility, Result, compose_native_matrix_ranking,
};

use super::{MAX_MATRIX_RESPONSE_BYTES, MatrixRankingBinding, native_wire};

/// Supplied by the caller, never read from process environment or saved audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JevNativeMatrixConfig {
    pub provider_identity: MatrixProviderIdentity,
    pub endpoint: Url,
    pub timeout: Duration,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
}

impl JevNativeMatrixConfig {
    fn validate(&self) -> Result<()> {
        self.provider_identity.provider_profile_ref.validate()?;
        self.provider_identity.model_configuration.validate()?;
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
            || !self.endpoint.username().is_empty()
            || self.endpoint.password().is_some()
            || self.endpoint.query().is_some()
            || self.endpoint.fragment().is_some()
            || self.endpoint.path() != native_wire::NATIVE_MATRIX_ENDPOINT_PATH
            || self.provider_identity.destination != self.endpoint.as_str()
            || self.provider_identity.destination.len() > 256
            || self.timeout.is_zero()
            || self.maximum_request_bytes == 0
            || self.maximum_request_bytes > MAX_PREPARED_MATRIX_BODY_BYTES
            || self.maximum_response_bytes == 0
            || self.maximum_response_bytes > MAX_MATRIX_RESPONSE_BYTES
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
}

pub struct JevNativeMatrixProvider {
    config: JevNativeMatrixConfig,
    client: reqwest::Client,
    authorization: HeaderValue,
}

impl JevNativeMatrixProvider {
    pub fn new(mut config: JevNativeMatrixConfig, bearer_credential: String) -> Result<Self> {
        config.provider_identity.wire_version = native_wire::NATIVE_MATRIX_WIRE_VERSION.into();
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

    fn parse_response(
        &self,
        binding: tect_application::MatrixProviderBinding,
        prepared: &native_wire::PreparedNativeMatrixRequest,
        bytes: Vec<u8>,
    ) -> Result<MatrixProviderResponse> {
        let parsed = native_wire::parse_native_response(
            &bytes,
            prepared,
            self.config.maximum_response_bytes,
        )?;
        let ranking = compose_native_matrix_ranking(&prepared.eligibility, &parsed.signals)?;
        Ok(MatrixProviderResponse {
            binding,
            provider_profile_ref: self.config.provider_identity.provider_profile_ref.clone(),
            model_configuration: self.config.provider_identity.model_configuration.clone(),
            response_payload_sha256: format!("{:x}", Sha256::digest(&bytes)),
            raw_response_payload: bytes,
            ranking,
            input_tokens: Some(parsed.input_tokens),
            output_tokens: Some(parsed.output_tokens),
        })
    }

    async fn bounded_body(&self, mut response: reqwest::Response) -> (Vec<u8>, bool) {
        let mut bytes = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    let remaining = self
                        .config
                        .maximum_response_bytes
                        .saturating_sub(bytes.len());
                    bytes.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
                    if chunk.len() > remaining {
                        return (bytes, false);
                    }
                }
                Ok(None) => return (bytes, true),
                Err(_) => return (bytes, false),
            }
        }
    }

    async fn send_once(&self, body: Vec<u8>) -> Result<MatrixProviderObservation> {
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
        let (bytes, response_complete) = self.bounded_body(response).await;
        Ok(MatrixProviderObservation {
            legacy_response: None,
            response_complete,
            response_payload: Some(bytes),
            http_status: Some(status.as_u16()),
            input_tokens: None,
            output_tokens: None,
        })
    }
}

#[async_trait]
impl MatrixAdviceProvider for JevNativeMatrixProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        Some(self.config.provider_identity.clone())
    }

    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        let native = native_wire::prepare_native_request(
            &self.config.provider_identity.model_configuration.model,
            request,
            self.config.maximum_request_bytes,
        )?;
        PreparedMatrixAdviceAttempt::new(
            request,
            self.config.provider_identity.clone(),
            native.body,
        )
    }

    fn parse_sealed_response(
        &self,
        request: &MatrixProviderRequest,
        saved: &StoredMatrixDispatch,
    ) -> Result<MatrixProviderResponse> {
        let native = native_wire::prepare_native_request(
            &self.config.provider_identity.model_configuration.model,
            request,
            self.config.maximum_request_bytes,
        )?;
        let response = saved
            .response_payload
            .as_ref()
            .ok_or(Error::InvalidArguments)?;
        if saved.request_payload.is_empty()
            || !saved.raw_observation_sealed
            || !saved.response_complete
            || !saved
                .response_http_status
                .is_some_and(|status| (200..300).contains(&status))
            || response.is_empty()
            || saved.request_payload.len() > self.config.maximum_request_bytes
            || response.len() > self.config.maximum_response_bytes
            || saved.dispatch.state != AdvisoryDispatchState::Sealed
            || saved.dispatch.send_certainty != AdvisorySendCertainty::Sent
            || saved.dispatch.outcome != Some(AdvisoryDispatchOutcome::ProviderResponse)
            || saved.dispatch.provider != self.config.provider_identity.provider_profile_ref.id
            || saved.dispatch.model != self.config.provider_identity.model_configuration.model
            || saved.dispatch.material_digest != request.binding().evaluation_digest
            || saved.binding != *request.binding()
            || saved.provider_profile_ref != self.config.provider_identity.provider_profile_ref
            || saved.model_configuration != self.config.provider_identity.model_configuration
            || saved.destination != self.config.provider_identity.destination
            || saved.wire_version != native_wire::NATIVE_MATRIX_WIRE_VERSION
            || saved.request_payload != native.body
        {
            return Err(Error::InvalidArguments);
        }
        let request_hash = format!("{:x}", Sha256::digest(&saved.request_payload));
        let response_hash = format!("{:x}", Sha256::digest(response));
        let snapshot = &saved.configuration_snapshot;
        let policy_id = snapshot
            .get("budget_policy_id")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidArguments)?;
        if policy_id.is_empty() || policy_id.len() > 256 || policy_id.trim() != policy_id {
            return Err(Error::InvalidArguments);
        }
        let expected_snapshot = json!({
            "provider_profile_ref": self.config.provider_identity.provider_profile_ref,
            "model_configuration": self.config.provider_identity.model_configuration,
            "destination": self.config.provider_identity.destination,
            "wire_version": native_wire::NATIVE_MATRIX_WIRE_VERSION,
            "budget_policy_id": policy_id,
            "request_body_length": saved.request_payload.len(),
            "request_body_sha256": request_hash,
            "budget_policy": validated_budget_header(snapshot, policy_id)?,
        });
        let snapshot_bytes = serde_json::to_vec(snapshot).map_err(|_| Error::InvalidArguments)?;
        if snapshot != &expected_snapshot
            || snapshot_bytes.len() > 8 * 1024
            || saved.request_payload_sha256 != request_hash
            || saved.response_payload_sha256.as_deref() != Some(response_hash.as_str())
            || saved.dispatch.payload_digest != request_hash
            || saved.dispatch.configuration_digest
                != format!("{:x}", Sha256::digest(snapshot_bytes))
        {
            return Err(Error::InvalidArguments);
        }
        let parsed = native_wire::parse_native_response(
            response,
            &native,
            self.config.maximum_response_bytes,
        )?;
        let input_tokens =
            i64::try_from(parsed.input_tokens).map_err(|_| Error::InvalidArguments)?;
        let output_tokens =
            i64::try_from(parsed.output_tokens).map_err(|_| Error::InvalidArguments)?;
        if saved
            .dispatch
            .input_tokens
            .is_some_and(|value| value != input_tokens)
            || saved
                .dispatch
                .output_tokens
                .is_some_and(|value| value != output_tokens)
        {
            return Err(Error::InvalidArguments);
        }
        let result = self.parse_response(request.binding().clone(), &native, response.clone())?;
        result.validate_for(request)?;
        Ok(result)
    }

    async fn attempt_prepared(
        &self,
        _prepared: PreparedMatrixAdviceAttempt,
        _permit: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        // Native transport requires the raw-seal lifecycle; the legacy typed
        // attempt cannot perform an unsealed parse or network call.
        Err(Error::TransportUnavailable)
    }

    async fn observe_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        permit: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderObservation> {
        // The permit is minted only after the Sending row commits and is consumed
        // before a single network attempt. A transport error has unknown send status.
        if !permit.permits_prepared(&prepared)
            || prepared.identity() != &self.config.provider_identity
            || prepared.body_length() > self.config.maximum_request_bytes
            || prepared.body_sha256() != format!("{:x}", Sha256::digest(prepared.body()))
        {
            return Err(Error::InputConflict);
        }
        native_from_prepared(&prepared)?;
        let (_, _, body, _) = prepared.into_parts();
        self.send_once(body).await
    }

    fn sealed_response_usage(&self, saved: &StoredMatrixDispatch) -> MatrixProviderUsage {
        let unknown = MatrixProviderUsage {
            input_tokens: None,
            output_tokens: None,
        };
        if !saved.raw_observation_sealed || !saved.response_complete {
            return unknown;
        }
        let Some(bytes) = saved.response_payload.as_ref() else {
            return unknown;
        };
        let Ok(value) = crate::jev_json::decode_unique_json(bytes) else {
            return unknown;
        };
        let Some(usage) = value.get("usage").and_then(Value::as_object) else {
            return unknown;
        };
        MatrixProviderUsage {
            input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
            output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
        }
    }
}

// Application owns signature verification and exact committed policy binding.
// The codec accepts only this known header; no arbitrary frozen fields are stripped.
fn validated_budget_header(snapshot: &Value, policy_id: &str) -> Result<Value> {
    let header = snapshot
        .get("budget_policy")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidArguments)?;
    if header.len() != 3
        || header.get("policy_id").and_then(Value::as_str) != Some(policy_id)
        || header
            .get("policy_version")
            .and_then(Value::as_i64)
            .is_none_or(|v| v <= 0)
        || !header
            .get("policy_digest")
            .and_then(Value::as_str)
            .is_some_and(|v| {
                v.len() == 64
                    && v.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
    {
        return Err(Error::InvalidArguments);
    }
    Ok(Value::Object(header.clone()))
}

/// Recover only parser metadata from the exact prepared native body. The body
/// and identity were fixed before budget authorization and checked by permit.
fn native_from_prepared(
    prepared: &PreparedMatrixAdviceAttempt,
) -> Result<native_wire::PreparedNativeMatrixRequest> {
    let value: Value =
        serde_json::from_slice(prepared.body()).map_err(|_| Error::InvalidArguments)?;
    let state = value.get("state").ok_or(Error::InvalidArguments)?;
    if state.get("contract").and_then(Value::as_str)
        != Some(native_wire::NATIVE_MATRIX_WIRE_VERSION)
        || value.get("model").and_then(Value::as_str)
            != Some(prepared.identity().model_configuration.model.as_str())
    {
        return Err(Error::InvalidArguments);
    }
    let binding: MatrixRankingBinding = serde_json::from_value(
        state
            .get("binding")
            .cloned()
            .ok_or(Error::InvalidArguments)?,
    )
    .map_err(|_| Error::InvalidArguments)?;
    let app_binding = prepared.binding();
    if binding.task_id != app_binding.task_id.to_string()
        || binding.task_revision != app_binding.task_revision.to_string()
        || binding.input_digest != app_binding.input_digest
        || binding.choice_set_id != app_binding.choice_set_id
        || binding.choice_set_version != app_binding.choice_set_version
        || binding.choice_set_digest != app_binding.choice_set_digest
        || binding.evaluation_digest != app_binding.evaluation_digest
        || binding.verification_digest != app_binding.verification_digest
    {
        return Err(Error::InputConflict);
    }
    let tokens = state
        .get("candidate_tokens")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidArguments)?;
    if !(2..=5).contains(&tokens.len()) {
        return Err(Error::InvalidArguments);
    }
    let mut token_to_candidate_id = BTreeMap::new();
    for (index, (token, candidate)) in tokens.iter().enumerate() {
        if token != &format!("C{index}") {
            return Err(Error::InvalidArguments);
        }
        let id = candidate
            .get("candidate_id")
            .and_then(Value::as_str)
            .ok_or(Error::InvalidArguments)?;
        if id.is_empty()
            || token_to_candidate_id
                .insert(token.clone(), id.to_owned())
                .is_some()
        {
            return Err(Error::InvalidArguments);
        }
    }
    let candidate_ids = token_to_candidate_id.values().cloned().collect::<Vec<_>>();
    if candidate_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != candidate_ids.len()
    {
        return Err(Error::InvalidArguments);
    }
    Ok(native_wire::PreparedNativeMatrixRequest {
        body: prepared.body().to_vec(),
        model: prepared.identity().model_configuration.model.clone(),
        binding,
        eligibility: MatrixAdviceEligibility::EligibleForAdvice { candidate_ids },
        token_to_candidate_id,
    })
}

#[cfg(test)]
#[path = "native_provider_tests.rs"]
mod tests;
