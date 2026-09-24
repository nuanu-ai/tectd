//! Pure Matrix ranking wire contract. Preparing or parsing does not dispatch Jev,
//! select an effective choice, or confer release authority.

mod wire;

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_application::{
    MatrixAdviceProvider, MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt, StoredMatrixDispatch,
};
use tect_domain::{
    AdvisoryDispatchOutcome, AdvisoryDispatchState, AdvisorySendCertainty, Error, Result,
};

const WIRE_VERSION: &str = "jev-matrix-ranking-json/2";

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
            || maximum_request_bytes == 0
            || maximum_response_bytes == 0
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
        let prepared = self.prepare(request)?;
        let dispatch = &saved.dispatch;
        let response = saved
            .response_payload
            .as_ref()
            .ok_or(Error::InvalidArguments)?;
        let response_hash = format!("{:x}", Sha256::digest(response));
        let request_hash = format!("{:x}", Sha256::digest(&saved.request_payload));
        let config_hash = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&saved.configuration_snapshot)
                    .map_err(|_| Error::InvalidArguments)?
            )
        );
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
        let wire = wire::prepare_verified_request(
            &self.identity.model_configuration.model,
            request,
            self.maximum_request_bytes,
        )?;
        if wire.body != saved.request_payload {
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
