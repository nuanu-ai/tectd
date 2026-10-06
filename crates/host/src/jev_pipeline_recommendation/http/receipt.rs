//! Native raw capture and pure sealed accounting. No answer interpretation at send.
use super::*;
use tect_application::AdvisoryProviderTransportContext;
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryDispatchOutcome, AdvisoryDispatchState,
    AdvisorySendCertainty,
};
use uuid::Uuid;

enum BodyRead {
    Complete(Vec<u8>),
    Partial(Vec<u8>, &'static str),
}

impl JevPipelineProvider {
    async fn receipt_body(&self, mut response: reqwest::Response) -> BodyRead {
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
                        return BodyRead::Partial(bytes, "response-oversize");
                    }
                }
                Ok(None) => return BodyRead::Complete(bytes),
                Err(_) => return BodyRead::Partial(bytes, "response-body-read"),
            }
        }
    }

    pub(super) async fn observe_once(
        &self,
        dispatch_id: Uuid,
        body: Vec<u8>,
    ) -> Result<AdvisoryProviderReceiptObservation> {
        let response = self
            .client
            .post(self.config.endpoint.clone())
            .header(AUTHORIZATION, self.authorization.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await;
        let response = match response {
            Ok(value) => value,
            Err(_) => {
                return Ok(observation(
                    self,
                    dispatch_id,
                    None,
                    None,
                    false,
                    Some("transport-unknown"),
                ));
            }
        };
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
        let (bytes, complete, failure) = match self.receipt_body(response).await {
            BodyRead::Partial(bytes, reason) => (bytes, false, Some(reason)),
            BodyRead::Complete(bytes) => {
                let reason = if !status.is_success() {
                    Some("http-status")
                } else if !is_json {
                    Some("content-type")
                } else if bytes.is_empty() {
                    Some("empty-response")
                } else {
                    None
                };
                (bytes, true, reason)
            }
        };
        Ok(observation(
            self,
            dispatch_id,
            Some(bytes),
            Some(status.as_u16()),
            complete,
            failure,
        ))
    }
}

fn observation(
    provider: &JevPipelineProvider,
    dispatch_id: Uuid,
    bytes: Option<Vec<u8>>,
    status: Option<u16>,
    complete: bool,
    failure: Option<&str>,
) -> AdvisoryProviderReceiptObservation {
    let received = bytes.is_some();
    let reference = match &bytes {
        Some(bytes) => format!(
            "jev:{}:{dispatch_id}:sha256:{:x}",
            provider.config.identity.provider,
            Sha256::digest(bytes)
        ),
        None => format!(
            "jev:{}:{dispatch_id}:transport-unknown",
            provider.config.identity.provider
        ),
    };
    AdvisoryProviderReceiptObservation {
        response_payload: bytes,
        http_status: status,
        input_tokens: None,
        output_tokens: None,
        response_complete: complete,
        original_transport_context: Some(AdvisoryProviderTransportContext {
            send_certainty: if received {
                AdvisorySendCertainty::Sent
            } else {
                AdvisorySendCertainty::SentUnknown
            },
            outcome: if failure.is_some() {
                AdvisoryDispatchOutcome::ProviderFailure
            } else {
                AdvisoryDispatchOutcome::ProviderResponse
            },
            raw_response_ref: Some(reference),
            provider_failure_code: failure.map(str::to_owned),
        }),
    }
}

/// Recover the entire manifest from the frozen request, then check canonical bytes.
pub(super) fn validate_body(
    provider: &JevPipelineProvider,
    bytes: &[u8],
    material_digest: &str,
) -> Result<PipelineRecommendationManifest> {
    let body = crate::jev_json::decode_unique_json(bytes).map_err(|_| Error::InputConflict)?;
    let manifest: PipelineRecommendationManifest = serde_json::from_value(
        body.get("state")
            .and_then(|state| state.get("manifest"))
            .cloned()
            .ok_or(Error::InputConflict)?,
    )
    .map_err(|_| Error::InputConflict)?;
    let native = prepare_native_request(
        &provider.config.identity.model,
        &manifest,
        provider.config.maximum_request_bytes,
    )?;
    if manifest.digest != material_digest || native.body != bytes {
        return Err(Error::InputConflict);
    }
    Ok(manifest)
}

pub(super) fn usage(
    provider: &JevPipelineProvider,
    saved: &StoredAdvisoryProviderReceipt,
) -> Result<AdvisoryProviderReceiptUsage> {
    let unknown = AdvisoryProviderReceiptUsage::default();
    let identity = &provider.config.identity;
    let expected = serde_json::json!({
        "provider_profile_ref": identity.provider,
        "model_configuration": {"model": identity.model},
        "destination": identity.destination,
        "wire_version": identity.wire_version,
        "request_body_sha256": saved.request_payload_sha256,
    });
    if !matches!(
        saved.dispatch.state,
        AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed
    ) || saved.dispatch.id.is_nil()
        || saved.opportunity.id.is_nil()
        || saved.dispatch.opportunity_id != saved.opportunity.id
        || saved.opportunity.capability != AdvisoryCapability::PipelineRecommendation
        || saved.opportunity.decision_point
            != AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
        || saved.opportunity.target_kind != "slice_candidate_node"
        || saved.opportunity.target_id.is_none()
        || saved.dispatch.material_digest != saved.opportunity.material_digest
        || saved.dispatch.provider != identity.provider
        || saved.dispatch.model != identity.model
        || saved.request_payload_sha256 != saved.dispatch.payload_digest
        || saved.request_payload_sha256 != format!("{:x}", Sha256::digest(&saved.request_payload))
        || saved.configuration_snapshot != expected
        || saved.dispatch.configuration_digest
            != format!(
                "{:x}",
                Sha256::digest(
                    serde_json::to_vec(&saved.configuration_snapshot)
                        .map_err(|_| Error::InputConflict)?
                )
            )
    {
        return Err(Error::InputConflict);
    }
    let manifest = validate_body(
        provider,
        &saved.request_payload,
        &saved.opportunity.material_digest,
    )?;
    if saved.opportunity.target_id != Some(manifest.work_id)
        || saved.opportunity.work_revision != Some(manifest.work_revision)
    {
        return Err(Error::InputConflict);
    }
    let Some(raw) = saved.observation.as_ref() else {
        return Ok(unknown);
    };
    let Some(context) = raw.original_transport_context.as_ref() else {
        return Err(Error::InputConflict);
    };
    context
        .validate_for(&raw.response_payload)
        .map_err(|_| Error::InputConflict)?;
    if !raw.response_complete {
        return Ok(unknown);
    }
    let Some(bytes) = raw.response_payload.as_ref() else {
        return Ok(unknown);
    };
    if bytes.len() > provider.config.maximum_response_bytes {
        return Ok(unknown);
    }
    let Ok(value) = crate::jev_json::decode_unique_json(bytes) else {
        return Ok(unknown);
    };
    let Some(tokens) = value.get("usage").and_then(Value::as_object) else {
        return Ok(unknown);
    };
    Ok(AdvisoryProviderReceiptUsage {
        input_tokens: tokens.get("input_tokens").and_then(Value::as_u64),
        output_tokens: tokens.get("output_tokens").and_then(Value::as_u64),
    })
}

#[cfg(test)]
mod tests;
