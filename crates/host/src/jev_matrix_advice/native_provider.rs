//! Explicitly constructed native TypeSafe Matrix adapter. Nothing in the host
//! installs it by default; the default Matrix provider and budget deny remain.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use reqwest::{
    Url,
    header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_application::{
    MAX_PREPARED_MATRIX_BODY_BYTES, MatrixAdviceProvider, MatrixProviderIdentity,
    MatrixProviderRequest, MatrixProviderResponse, MatrixStartedDispatchPermit,
    PreparedMatrixAdviceAttempt, StoredMatrixDispatch,
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
        if !matches!(self.endpoint.scheme(), "http" | "https")
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

    async fn bounded_body(&self, mut response: reqwest::Response) -> Result<Vec<u8>> {
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
        if bytes.is_empty() {
            return Err(Error::InvalidArguments);
        }
        Ok(bytes)
    }

    async fn send_once(&self, body: Vec<u8>) -> Result<Vec<u8>> {
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
        let bytes = self.bounded_body(response).await?;
        if !status.is_success() || !is_json {
            return Err(Error::TransportUnavailable);
        }
        Ok(bytes)
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
        if saved.dispatch.input_tokens != Some(input_tokens)
            || saved.dispatch.output_tokens != Some(output_tokens)
        {
            return Err(Error::InvalidArguments);
        }
        let result = self.parse_response(request.binding().clone(), &native, response.clone())?;
        result.validate_for(request)?;
        Ok(result)
    }

    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        permit: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        // The permit is minted only after the Sending row commits and is consumed
        // before a single network attempt. A transport error has unknown send status.
        if !permit.permits_prepared(&prepared)
            || prepared.identity() != &self.config.provider_identity
            || prepared.body_length() > self.config.maximum_request_bytes
            || prepared.body_sha256() != format!("{:x}", Sha256::digest(prepared.body()))
        {
            return Err(Error::InputConflict);
        }
        let native = native_from_prepared(&prepared)?;
        let (binding, _, body, _) = prepared.into_parts();
        let bytes = self.send_once(body).await?;
        self.parse_response(binding, &native, bytes)
    }
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
        || binding.choice_set_version
            != u64::try_from(app_binding.choice_set_version).map_err(|_| Error::InvalidArguments)?
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
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef};

    fn config(endpoint: Url, maximum_response_bytes: usize) -> JevNativeMatrixConfig {
        JevNativeMatrixConfig {
            provider_identity: MatrixProviderIdentity {
                provider_profile_ref: AdvisoryProviderProfileRef {
                    id: "profile".into(),
                },
                model_configuration: AdvisoryModelConfiguration {
                    model: "jev-1.13.0".into(),
                },
                destination: endpoint.as_str().into(),
                wire_version: "caller-value-is-normalized".into(),
            },
            endpoint,
            timeout: Duration::from_secs(2),
            maximum_request_bytes: 262_144,
            maximum_response_bytes,
        }
    }

    fn loopback(response: Vec<u8>) -> (Url, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = Url::parse(&format!(
            "http://{}/v1/systemone",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = vec![0; 4096];
            let received = stream.read(&mut request).unwrap();
            stream.write_all(&response).unwrap();
            String::from_utf8_lossy(&request[..received]).into_owned()
        });
        (endpoint, handle)
    }

    #[test]
    fn constructor_requires_explicit_credential_and_exact_endpoint_path() {
        let endpoint = Url::parse("http://127.0.0.1:9/v1/systemone").unwrap();
        assert!(matches!(
            JevNativeMatrixProvider::new(config(endpoint.clone(), 1024), String::new()),
            Err(Error::InvalidConfiguration)
        ));
        let wrong = Url::parse("http://127.0.0.1:9/other").unwrap();
        assert!(matches!(
            JevNativeMatrixProvider::new(config(wrong, 1024), "secret".into()),
            Err(Error::InvalidConfiguration)
        ));
        let provider =
            JevNativeMatrixProvider::new(config(endpoint, 1024), "secret".into()).unwrap();
        assert_eq!(
            provider.identity().unwrap().wire_version,
            native_wire::NATIVE_MATRIX_WIRE_VERSION
        );
    }

    #[tokio::test]
    async fn single_loopback_post_has_bearer_and_returns_bounded_bytes() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec();
        let (endpoint, server) = loopback(response);
        let provider =
            JevNativeMatrixProvider::new(config(endpoint, 64), "local-test".into()).unwrap();
        assert_eq!(provider.send_once(b"{}".to_vec()).await, Ok(b"{}".to_vec()));
        let request = server.join().unwrap();
        assert!(request.starts_with("POST /v1/systemone HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer local-test")
        );
    }

    #[tokio::test]
    async fn oversized_loopback_response_is_rejected_while_reading() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345".to_vec();
        let (endpoint, server) = loopback(response);
        let provider =
            JevNativeMatrixProvider::new(config(endpoint, 4), "local-test".into()).unwrap();
        assert_eq!(
            provider.send_once(b"{}".to_vec()).await,
            Err(Error::RequestTooLarge)
        );
        server.join().unwrap();
    }
}
