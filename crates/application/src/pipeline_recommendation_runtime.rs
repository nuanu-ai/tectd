//! Slice 03 provider boundary. Preparing a recommendation is never permission to send.

use crate::PreparedPipelineRecommendation;
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryCapability, AdvisoryDispatch, AdvisoryDispatchAuthorization, AdvisoryDispatchOutcome,
    AdvisoryDispatchStart, AdvisoryDispatchState, AdvisoryOpportunityState, AdvisorySendCertainty,
    Error, PipelineRecommendationManifest, PipelineRecommendationRanking, Result,
};
use uuid::Uuid;

pub const MAX_PREPARED_PIPELINE_BODY_BYTES: usize = 512 * 1024;
pub const MAX_SEALED_PIPELINE_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineProviderIdentity {
    pub provider: String,
    pub model: String,
    pub destination: String,
    pub wire_version: String,
}

impl PipelineProviderIdentity {
    fn validate(&self) -> Result<()> {
        if [
            &self.provider,
            &self.model,
            &self.destination,
            &self.wire_version,
        ]
        .iter()
        .any(|value| value.is_empty() || value.len() > 256 || value.chars().any(char::is_control))
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

/// Immutable exact request. The manifest digest and request hash are fixed
/// before a dispatch can be authorized or started.
#[derive(Debug, PartialEq, Eq)]
pub struct PreparedPipelineRecommendationAttempt {
    opportunity_id: Uuid,
    manifest_digest: String,
    identity: PipelineProviderIdentity,
    body: Vec<u8>,
    body_sha256: String,
}

impl PreparedPipelineRecommendationAttempt {
    pub fn new(
        prepared: &PreparedPipelineRecommendation,
        identity: PipelineProviderIdentity,
        body: Vec<u8>,
    ) -> Result<Self> {
        prepared.manifest.validate_digest()?;
        identity.validate()?;
        if prepared.opportunity.id.is_nil()
            || prepared.opportunity.capability != AdvisoryCapability::PipelineRecommendation
            || prepared.opportunity.material_digest != prepared.manifest.digest
            || prepared.context.verification_contract_digest != prepared.manifest.digest
            || !matches!(
                prepared.opportunity.state,
                AdvisoryOpportunityState::Prepared
                    | AdvisoryOpportunityState::AwaitingResponse
                    | AdvisoryOpportunityState::Advised
                    | AdvisoryOpportunityState::Failed
                    | AdvisoryOpportunityState::Invalidated
                    | AdvisoryOpportunityState::Unresolved
            )
            || !prepared.manifest.should_call()
        {
            return Err(Error::InputConflict);
        }
        if body.is_empty() || body.len() > MAX_PREPARED_PIPELINE_BODY_BYTES {
            return Err(Error::RequestTooLarge);
        }
        if std::str::from_utf8(&body).is_err() {
            return Err(Error::InvalidArguments);
        }
        Ok(Self {
            opportunity_id: prepared.opportunity.id,
            manifest_digest: prepared.manifest.digest.clone(),
            identity,
            body_sha256: format!("{:x}", Sha256::digest(&body)),
            body,
        })
    }

    pub fn opportunity_id(&self) -> Uuid {
        self.opportunity_id
    }
    pub fn manifest_digest(&self) -> &str {
        &self.manifest_digest
    }
    pub fn identity(&self) -> &PipelineProviderIdentity {
        &self.identity
    }
    pub fn body(&self) -> &[u8] {
        &self.body
    }
    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }
}

/// One-use token minted only after the dispatch-start transaction commits.
pub struct PipelineStartedDispatchPermit {
    dispatch_id: Uuid,
    opportunity_id: Uuid,
    manifest_digest: String,
    request_digest: String,
    identity: PipelineProviderIdentity,
}

impl PipelineStartedDispatchPermit {
    pub fn dispatch_id(&self) -> Uuid {
        self.dispatch_id
    }
    pub(crate) fn after_committed_start(
        started: &AdvisoryDispatchStart,
        authorization: &AdvisoryDispatchAuthorization,
        prepared: &PreparedPipelineRecommendationAttempt,
    ) -> Result<Self> {
        let dispatch = &started.dispatch;
        let config_bytes = serde_json::to_vec(&authorization.configuration_snapshot)
            .map_err(Error::invalid_arguments_from)?;
        let config_digest = format!("{:x}", Sha256::digest(config_bytes));
        let config = &authorization.configuration_snapshot;
        let reservation = started
            .budget_reservation
            .as_ref()
            .ok_or(Error::BudgetPolicyInvalid)?;
        if !started.should_send
            || reservation.dispatch_id != dispatch.id
            || reservation.request_sha256 != prepared.body_sha256
            || reservation.request_utf8_bytes
                != i64::try_from(prepared.body.len()).map_err(|_| Error::BudgetPolicyInvalid)?
            || reservation.reserved_calls != 1
            || reservation.reserved_retry_dispatches != 0
            || reservation.policy_version <= 0
            || reservation.policy_digest.len() != 64
            || !reservation
                .policy_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || reservation.policy_effective_from_unix_ms
                >= reservation.policy_effective_until_unix_ms
            || reservation.remaining_elapsed_ms <= 0
            || reservation.reserved_input_tokens <= 0
            || reservation.reserved_output_tokens <= 0
            || dispatch.state != AdvisoryDispatchState::Sending
            || dispatch.send_certainty != AdvisorySendCertainty::SentUnknown
            || dispatch.id != authorization.dispatch_id
            || dispatch.opportunity_id != prepared.opportunity_id
            || authorization.opportunity_id != prepared.opportunity_id
            || dispatch.provider != prepared.identity.provider
            || dispatch.model != prepared.identity.model
            || authorization.provider != prepared.identity.provider
            || authorization.model != prepared.identity.model
            || dispatch.material_digest != prepared.manifest_digest
            || authorization.material_digest != prepared.manifest_digest
            || dispatch.payload_digest != prepared.body_sha256
            || authorization.payload_digest != prepared.body_sha256
            || authorization.request_payload != prepared.body
            || dispatch.configuration_digest != config_digest
            || authorization.configuration_digest != config_digest
            || config.get("destination") != Some(&serde_json::json!(prepared.identity.destination))
            || config.get("wire_version")
                != Some(&serde_json::json!(prepared.identity.wire_version))
            || config.get("request_body_sha256") != Some(&serde_json::json!(prepared.body_sha256))
            || authorization.validate().is_err()
        {
            return Err(Error::InputConflict);
        }
        Ok(Self {
            dispatch_id: dispatch.id,
            opportunity_id: prepared.opportunity_id,
            manifest_digest: prepared.manifest_digest.clone(),
            request_digest: prepared.body_sha256.clone(),
            identity: prepared.identity.clone(),
        })
    }

    /// Consumes the token at the only provider send entry point.
    pub fn permits(self, prepared: &PreparedPipelineRecommendationAttempt) -> bool {
        !self.dispatch_id.is_nil()
            && self.opportunity_id == prepared.opportunity_id
            && self.manifest_digest == prepared.manifest_digest
            && self.request_digest == prepared.body_sha256
            && self.identity == prepared.identity
    }
}

/// Bytes read from a sealed durable dispatch, validated against the exact
/// prepared request. A raw network response cannot construct this value.
pub struct SealedPipelineRecommendationResponse {
    dispatch_id: Uuid,
    bytes: Vec<u8>,
    sha256: String,
}

impl SealedPipelineRecommendationResponse {
    pub fn from_saved(
        dispatch: &AdvisoryDispatch,
        prepared: &PreparedPipelineRecommendationAttempt,
        saved_request: &[u8],
        response: Vec<u8>,
        saved_response_sha256: &str,
    ) -> Result<Self> {
        let sha256 = format!("{:x}", Sha256::digest(&response));
        if dispatch.state != AdvisoryDispatchState::Sealed
            || dispatch.send_certainty != AdvisorySendCertainty::Sent
            || dispatch.outcome != Some(AdvisoryDispatchOutcome::ProviderResponse)
            || dispatch.opportunity_id != prepared.opportunity_id
            || dispatch.provider != prepared.identity.provider
            || dispatch.model != prepared.identity.model
            || dispatch.material_digest != prepared.manifest_digest
            || dispatch.payload_digest != prepared.body_sha256
            || dispatch.id.is_nil()
            || format!("{:x}", Sha256::digest(saved_request)) != prepared.body_sha256
            || saved_request != prepared.body
            || response.is_empty()
            || response.len() > MAX_SEALED_PIPELINE_RESPONSE_BYTES
            || sha256 != saved_response_sha256
        {
            return Err(Error::InputConflict);
        }
        Ok(Self {
            dispatch_id: dispatch.id,
            bytes: response,
            sha256,
        })
    }

    pub fn dispatch_id(&self) -> Uuid {
        self.dispatch_id
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct PipelineProviderObservation {
    pub raw_response: Vec<u8>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

#[async_trait]
pub trait PipelineRecommendationProvider: Send + Sync {
    async fn observe_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<crate::AdvisoryProviderReceiptObservation> {
        let observation = self.attempt_prepared(prepared, permit).await?;
        Ok(crate::AdvisoryProviderReceiptObservation {
            response_payload: Some(observation.raw_response),
            http_status: None,
            input_tokens: observation.input_tokens,
            output_tokens: observation.output_tokens,
            response_complete: true,
            original_transport_context: Some(crate::AdvisoryProviderTransportContext {
                send_certainty: AdvisorySendCertainty::Sent,
                outcome: AdvisoryDispatchOutcome::ProviderResponse,
                raw_response_ref: None,
                provider_failure_code: None,
            }),
        })
    }

    fn usage_from_sealed_response(
        &self,
        saved: &crate::StoredAdvisoryProviderReceipt,
    ) -> Result<crate::AdvisoryProviderReceiptUsage> {
        Ok(crate::scope_advisory_provider_receipt::original_usage(
            saved,
        ))
    }

    fn available(&self) -> bool {
        true
    }

    fn prepare(
        &self,
        saved: &PreparedPipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendationAttempt>;

    fn parse_sealed_response(
        &self,
        manifest: &PipelineRecommendationManifest,
        prepared: &PreparedPipelineRecommendationAttempt,
        sealed: &SealedPipelineRecommendationResponse,
    ) -> Result<PipelineRecommendationRanking>;

    async fn attempt_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<PipelineProviderObservation>;
}

mod disabled_provider;
pub use disabled_provider::DisabledPipelineRecommendationProvider;

#[cfg(test)]
mod tests;
