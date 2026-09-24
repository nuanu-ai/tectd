//! Pure Matrix ranking wire contract. Preparing or parsing does not dispatch Jev,
//! select an effective choice, or confer release authority.

pub mod native_provider;
pub mod native_wire;
mod wire;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_application::{
    MAX_PREPARED_MATRIX_BODY_BYTES, MatrixAdviceProvider, MatrixProviderIdentity,
    MatrixProviderRequest, MatrixProviderResponse, MatrixStartedDispatchPermit,
    PreparedMatrixAdviceAttempt, StoredMatrixDispatch,
};
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisoryDispatchState, AdvisorySendCertainty, Error, Result,
};

const WIRE_VERSION: &str = "jev-matrix-ranking-json/2";
const MAX_MATRIX_CONFIGURATION_TEXT_BYTES: usize = 256;
// The exact seven-key snapshot shape below bounds every serialized string to
// 256 bytes (or 64 hex digits), keeping JSON escaping below this cap.
const MAX_MATRIX_CONFIGURATION_BYTES: usize = 8 * 1024;
const MAX_MATRIX_RESPONSE_BYTES: usize = crate::frame::MAX_FRAME_BYTES;

/// Read-only TypeSafe Matrix v2 adapter. It has no HTTP client or credential.
/// Installing a sending provider requires a separate transport implementation.
pub struct JevMatrixSavedResponseParser {
    identity: MatrixProviderIdentity,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
}

impl JevMatrixSavedResponseParser {
    pub fn new(
        mut identity: MatrixProviderIdentity,
        maximum_request_bytes: usize,
        maximum_response_bytes: usize,
    ) -> Result<Self> {
        identity.provider_profile_ref.validate()?;
        identity.model_configuration.validate()?;
        if identity.destination.is_empty()
            || identity.destination.contains('\0')
            || identity.destination.len() > MAX_MATRIX_CONFIGURATION_TEXT_BYTES
            || maximum_request_bytes == 0
            || maximum_request_bytes > MAX_PREPARED_MATRIX_BODY_BYTES
            || maximum_response_bytes == 0
            || maximum_response_bytes > MAX_MATRIX_RESPONSE_BYTES
        {
            return Err(Error::InvalidArguments);
        }
        identity.wire_version = WIRE_VERSION.into();
        Ok(Self {
            identity,
            maximum_request_bytes,
            maximum_response_bytes,
        })
    }
}

#[async_trait]
impl MatrixAdviceProvider for JevMatrixSavedResponseParser {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        // This adapter is never eligible for a new dispatch.
        None
    }

    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        let wire = wire::prepare_verified_request(
            &self.identity.model_configuration.model,
            request,
            self.maximum_request_bytes,
        )?;
        PreparedMatrixAdviceAttempt::new(request, self.identity.clone(), wire.body)
    }

    fn parse_sealed_response(
        &self,
        request: &MatrixProviderRequest,
        saved: &StoredMatrixDispatch,
    ) -> Result<MatrixProviderResponse> {
        validate_saved_dispatch_bounds(
            saved,
            &self.identity,
            self.maximum_request_bytes,
            self.maximum_response_bytes,
        )?;

        let wire = wire::prepare_verified_request(
            &self.identity.model_configuration.model,
            request,
            self.maximum_request_bytes,
        )?;
        let prepared =
            PreparedMatrixAdviceAttempt::new(request, self.identity.clone(), wire.body.clone())?;
        let dispatch = &saved.dispatch;
        let response = saved
            .response_payload
            .as_ref()
            .ok_or(Error::InvalidArguments)?;
        let response_hash = format!("{:x}", Sha256::digest(response));
        let request_hash = format!("{:x}", Sha256::digest(&saved.request_payload));
        let configuration_bytes = bounded_configuration_snapshot_bytes(
            &saved.configuration_snapshot,
            &self.identity,
            saved.request_payload.len(),
        )?;
        let config_hash = format!("{:x}", Sha256::digest(&configuration_bytes));
        let config = &saved.configuration_snapshot;
        if dispatch.state != AdvisoryDispatchState::Sealed
            || dispatch.send_certainty != AdvisorySendCertainty::Sent
            || dispatch.outcome != Some(AdvisoryDispatchOutcome::ProviderResponse)
            || dispatch.provider != self.identity.provider_profile_ref.id
            || dispatch.model != self.identity.model_configuration.model
            || dispatch.material_digest != request.binding().evaluation_digest
            || dispatch.payload_digest != request_hash
            || dispatch.configuration_digest != config_hash
            || saved.binding != *request.binding()
            || saved.provider_profile_ref != self.identity.provider_profile_ref
            || saved.model_configuration != self.identity.model_configuration
            || saved.destination != self.identity.destination
            || saved.wire_version != WIRE_VERSION
            || saved.request_payload != prepared.body()
            || saved.request_payload_sha256 != request_hash
            || saved.response_payload_sha256.as_deref() != Some(response_hash.as_str())
            || config.get("provider_profile_ref")
                != Some(&serde_json::json!(self.identity.provider_profile_ref))
            || config.get("model_configuration")
                != Some(&serde_json::json!(self.identity.model_configuration))
            || config.get("destination") != Some(&serde_json::json!(self.identity.destination))
            || config.get("wire_version") != Some(&serde_json::json!(WIRE_VERSION))
            || config.get("request_body_length") != Some(&serde_json::json!(prepared.body_length()))
            || config.get("request_body_sha256") != Some(&serde_json::json!(prepared.body_sha256()))
        {
            return Err(Error::InvalidArguments);
        }
        let parsed = parse_saved_wire_response(
            response,
            &response_hash,
            &wire,
            self.maximum_response_bytes,
        )?;
        let input_tokens = parsed
            .input_tokens
            .map(i64::try_from)
            .transpose()
            .map_err(|_| Error::InvalidArguments)?;
        let output_tokens = parsed
            .output_tokens
            .map(i64::try_from)
            .transpose()
            .map_err(|_| Error::InvalidArguments)?;
        if input_tokens != dispatch.input_tokens || output_tokens != dispatch.output_tokens {
            return Err(Error::InvalidArguments);
        }
        let result = MatrixProviderResponse {
            binding: request.binding().clone(),
            provider_profile_ref: self.identity.provider_profile_ref.clone(),
            model_configuration: self.identity.model_configuration.clone(),
            raw_response_payload: response.clone(),
            response_payload_sha256: response_hash,
            ranking: parsed.ranking,
            input_tokens: parsed.input_tokens,
            output_tokens: parsed.output_tokens,
        };
        result.validate_for(request)?;
        Ok(result)
    }

    async fn attempt_prepared(
        &self,
        _: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        Err(Error::TransportUnavailable)
    }
}

fn validate_saved_dispatch_bounds(
    saved: &StoredMatrixDispatch,
    identity: &MatrixProviderIdentity,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
) -> Result<()> {
    let response = saved
        .response_payload
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    if saved.request_payload.is_empty() || response.is_empty() {
        return Err(Error::InvalidArguments);
    }
    if saved.request_payload.len() > maximum_request_bytes
        || saved.request_payload.len() > MAX_PREPARED_MATRIX_BODY_BYTES
        || response.len() > maximum_response_bytes
        || response.len() > MAX_MATRIX_RESPONSE_BYTES
    {
        return Err(Error::RequestTooLarge);
    }
    if !has_bounded_configuration_snapshot(
        &saved.configuration_snapshot,
        identity,
        saved.request_payload.len(),
    ) {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn has_bounded_configuration_snapshot(
    snapshot: &serde_json::Value,
    identity: &MatrixProviderIdentity,
    request_payload_len: usize,
) -> bool {
    let Some(snapshot) = snapshot.as_object() else {
        return false;
    };
    if snapshot.len() != 7 {
        return false;
    }

    let Some(profile) = snapshot
        .get("provider_profile_ref")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    let Some(profile_id) = profile.get("id").and_then(serde_json::Value::as_str) else {
        return false;
    };
    if profile.len() != 1
        || profile_id.len() > MAX_MATRIX_CONFIGURATION_TEXT_BYTES
        || profile_id != identity.provider_profile_ref.id
    {
        return false;
    }

    let Some(model_configuration) = snapshot
        .get("model_configuration")
        .and_then(serde_json::Value::as_object)
    else {
        return false;
    };
    let Some(model) = model_configuration
        .get("model")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    if model_configuration.len() != 1
        || model.len() > MAX_MATRIX_CONFIGURATION_TEXT_BYTES
        || model != identity.model_configuration.model
    {
        return false;
    }

    let Some(destination) = snapshot
        .get("destination")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let Some(wire_version) = snapshot
        .get("wire_version")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let Some(budget_policy_id) = snapshot
        .get("budget_policy_id")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    if !bounded_configuration_text(destination)
        || destination != identity.destination
        || !bounded_configuration_text(wire_version)
        || wire_version != WIRE_VERSION
        || !bounded_configuration_text(budget_policy_id)
        || budget_policy_id.trim() != budget_policy_id
    {
        return false;
    }

    let Some(configured_request_len) = snapshot
        .get("request_body_length")
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
    else {
        return false;
    };
    let Some(request_body_sha256) = snapshot
        .get("request_body_sha256")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    configured_request_len == request_payload_len
        && request_body_sha256.len() == 64
        && request_body_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn bounded_configuration_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_MATRIX_CONFIGURATION_TEXT_BYTES && !value.contains('\0')
}

fn bounded_configuration_snapshot_bytes(
    snapshot: &serde_json::Value,
    identity: &MatrixProviderIdentity,
    request_payload_len: usize,
) -> Result<Vec<u8>> {
    if !has_bounded_configuration_snapshot(snapshot, identity, request_payload_len) {
        return Err(Error::InvalidArguments);
    }
    let bytes = serde_json::to_vec(snapshot).map_err(|_| Error::InvalidArguments)?;
    if bytes.len() > MAX_MATRIX_CONFIGURATION_BYTES {
        return Err(Error::RequestTooLarge);
    }
    Ok(bytes)
}

fn parse_saved_wire_response(
    bytes: &[u8],
    saved_sha256: &str,
    prepared: &PreparedMatrixRankingRequest,
    maximum_response_bytes: usize,
) -> Result<ParsedMatrixRankingResponse> {
    if saved_sha256 != format!("{:x}", Sha256::digest(bytes)) {
        return Err(Error::InvalidArguments);
    }
    wire::parse_response(bytes, &prepared.model, prepared, maximum_response_bytes)
}

pub use wire::{
    MatrixRankingBinding, ParsedMatrixRankingResponse, PreparedMatrixRankingRequest,
    parse_response, prepare_request, prepare_verified_request,
};

#[cfg(test)]
#[path = "jev_matrix_advice/saved_response_tests.rs"]
mod saved_response_tests;
