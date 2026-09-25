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
            || prepared.opportunity.state != AdvisoryOpportunityState::Prepared
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
        if !started.should_send
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

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::AdvisoryRetryBasis;

    struct FakeProvider;

    #[async_trait]
    impl PipelineRecommendationProvider for FakeProvider {
        fn prepare(
            &self,
            _: &PreparedPipelineRecommendation,
        ) -> Result<PreparedPipelineRecommendationAttempt> {
            Err(Error::TransportUnavailable)
        }

        fn parse_sealed_response(
            &self,
            _: &PipelineRecommendationManifest,
            _: &PreparedPipelineRecommendationAttempt,
            _: &SealedPipelineRecommendationResponse,
        ) -> Result<PipelineRecommendationRanking> {
            Ok(PipelineRecommendationRanking::Abstained)
        }

        async fn attempt_prepared(
            &self,
            prepared: PreparedPipelineRecommendationAttempt,
            permit: PipelineStartedDispatchPermit,
        ) -> Result<PipelineProviderObservation> {
            if !permit.permits(&prepared) {
                return Err(Error::InputConflict);
            }
            Ok(PipelineProviderObservation {
                raw_response: b"fake response".to_vec(),
                input_tokens: Some(1),
                output_tokens: Some(2),
            })
        }
    }

    fn identity() -> PipelineProviderIdentity {
        PipelineProviderIdentity {
            provider: "fake-jev".into(),
            model: "jev-test".into(),
            destination: "https://example.invalid/v1/systemone".into(),
            wire_version: "tect.pipeline-typesafe-native/1".into(),
        }
    }

    fn attempt() -> PreparedPipelineRecommendationAttempt {
        let body = b"exact request".to_vec();
        PreparedPipelineRecommendationAttempt {
            opportunity_id: Uuid::new_v4(),
            manifest_digest: "a".repeat(64),
            identity: identity(),
            body_sha256: format!("{:x}", Sha256::digest(&body)),
            body,
        }
    }

    fn dispatch(attempt: &PreparedPipelineRecommendationAttempt) -> AdvisoryDispatch {
        AdvisoryDispatch {
            id: Uuid::new_v4(),
            opportunity_id: attempt.opportunity_id,
            predecessor_dispatch_id: None,
            attempt_number: 1,
            provider: attempt.identity.provider.clone(),
            model: attempt.identity.model.clone(),
            configuration_digest: "b".repeat(64),
            material_digest: attempt.manifest_digest.clone(),
            payload_digest: attempt.body_sha256.clone(),
            input_tokens: None,
            output_tokens: None,
            latency_ms: None,
            state: AdvisoryDispatchState::Authorized,
            send_certainty: AdvisorySendCertainty::NotSent,
            outcome: None,
            retry_basis: AdvisoryRetryBasis::Initial,
            raw_response_ref: None,
        }
    }

    #[tokio::test]
    async fn prepared_or_authorized_row_cannot_enter_provider_or_parser() {
        let attempt = attempt();
        let authorized = dispatch(&attempt);
        let config = serde_json::json!({
            "destination": attempt.identity.destination,
            "wire_version": attempt.identity.wire_version,
            "request_body_sha256": attempt.body_sha256,
        });
        let authorization = AdvisoryDispatchAuthorization {
            dispatch_id: authorized.id,
            opportunity_id: attempt.opportunity_id,
            predecessor_dispatch_id: None,
            attempt_number: 1,
            retry_basis: AdvisoryRetryBasis::Initial,
            provider: attempt.identity.provider.clone(),
            model: attempt.identity.model.clone(),
            configuration_digest: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&config).unwrap())
            ),
            configuration_snapshot: config,
            material_digest: attempt.manifest_digest.clone(),
            payload_digest: attempt.body_sha256.clone(),
            request_payload: attempt.body.clone(),
        };
        let start = AdvisoryDispatchStart {
            dispatch: authorized.clone(),
            should_send: true,
        };
        assert!(
            PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
                .is_err()
        );
        assert!(
            SealedPipelineRecommendationResponse::from_saved(
                &authorized,
                &attempt,
                &attempt.body,
                b"response".to_vec(),
                &format!("{:x}", Sha256::digest(b"response")),
            )
            .is_err()
        );

        let mut sending = authorized;
        sending.state = AdvisoryDispatchState::Sending;
        sending.send_certainty = AdvisorySendCertainty::SentUnknown;
        sending.configuration_digest = authorization.configuration_digest.clone();
        let start = AdvisoryDispatchStart {
            dispatch: sending.clone(),
            should_send: true,
        };
        let permit =
            PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
                .unwrap();
        let observed = FakeProvider.attempt_prepared(self::attempt(), permit).await;
        assert!(matches!(observed, Err(Error::InputConflict)));

        let permit =
            PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
                .unwrap();
        let observed = FakeProvider
            .attempt_prepared(attempt, permit)
            .await
            .unwrap();
        assert_eq!(observed.raw_response, b"fake response");

        let attempt = PreparedPipelineRecommendationAttempt {
            opportunity_id: sending.opportunity_id,
            manifest_digest: sending.material_digest.clone(),
            identity: identity(),
            body_sha256: sending.payload_digest.clone(),
            body: b"exact request".to_vec(),
        };

        sending.state = AdvisoryDispatchState::Sealed;
        sending.send_certainty = AdvisorySendCertainty::Sent;
        sending.outcome = Some(AdvisoryDispatchOutcome::ProviderResponse);
        assert!(
            SealedPipelineRecommendationResponse::from_saved(
                &sending,
                &attempt,
                b"wrong request",
                b"response".to_vec(),
                &format!("{:x}", Sha256::digest(b"response")),
            )
            .is_err()
        );
        assert!(
            SealedPipelineRecommendationResponse::from_saved(
                &sending,
                &attempt,
                &attempt.body,
                b"response".to_vec(),
                "wrong digest",
            )
            .is_err()
        );
        assert!(
            SealedPipelineRecommendationResponse::from_saved(
                &sending,
                &attempt,
                &attempt.body,
                b"response".to_vec(),
                &format!("{:x}", Sha256::digest(b"response")),
            )
            .is_ok()
        );
    }
}
