use super::*;
use serde_json::{Value, json};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryDispatchState, NormalizedScopeAdviceAnswers,
    ScopeDecompositionAlternative,
};

pub(super) fn usage(
    provider: &JevScopeAdviceProvider,
    saved: &StoredAdvisoryProviderReceipt,
) -> Result<AdvisoryProviderReceiptUsage> {
    let unknown = AdvisoryProviderReceiptUsage::default();
    let Some(raw) = saved.observation.as_ref() else {
        return Ok(unknown);
    };
    if !matches!(
        saved.dispatch.state,
        AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed
    ) || !raw.response_complete
    {
        return Ok(unknown);
    }
    let Some(bytes) = raw.response_payload.as_ref() else {
        return Ok(unknown);
    };
    if saved.dispatch.provider != "jev-system-one"
        || saved.dispatch.model != provider.config.model
        || saved.dispatch.opportunity_id != saved.opportunity.id
        || saved.opportunity.capability != AdvisoryCapability::ScopeDecomposition
        || saved.configuration_snapshot.get("provider_profile_ref")
            != Some(&json!({"id":provider.config.profile}))
        || saved
            .configuration_snapshot
            .get("destination")
            .and_then(Value::as_str)
            != Some(provider.config.endpoint.as_str())
        || saved
            .configuration_snapshot
            .get("wire_version")
            .and_then(Value::as_str)
            != Some(WIRE_FORMAT)
        || saved.request_payload_sha256 != saved.dispatch.payload_digest
        || saved.request_payload_sha256 != format!("{:x}", Sha256::digest(&saved.request_payload))
        || raw
            .original_transport_context
            .as_ref()
            .is_none_or(|context| context.send_certainty != AdvisorySendCertainty::Sent)
    {
        return Err(Error::InputConflict);
    }
    if bytes.len() > provider.config.maximum_response_bytes {
        return Ok(unknown);
    }
    let Ok(value) = wire::parse_unique_json(bytes) else {
        return Ok(unknown);
    };
    let Some(usage) = value.get("usage").and_then(Value::as_object) else {
        return Ok(unknown);
    };
    Ok(AdvisoryProviderReceiptUsage {
        input_tokens: usage.get("input_tokens").and_then(Value::as_u64),
        output_tokens: usage.get("output_tokens").and_then(Value::as_u64),
    })
}

pub(super) fn parse(
    provider: &JevScopeAdviceProvider,
    prepared: &PreparedScopeAdviceAttempt,
    saved: &StoredAdvisoryProviderReceipt,
) -> Result<NormalizedScopeAdviceAnswers> {
    let raw = saved.observation.as_ref().ok_or(Error::InvalidArguments)?;
    let bytes = raw
        .response_payload
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let context = raw
        .original_transport_context
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let snapshot = &saved.configuration_snapshot;
    let policy_id = snapshot
        .get("budget_policy_id")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidArguments)?;
    let budget = snapshot
        .get("budget_policy")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidArguments)?;
    if policy_id.is_empty()
        || budget.len() != 3
        || budget.get("policy_id").and_then(Value::as_str) != Some(policy_id)
        || budget
            .get("policy_version")
            .and_then(Value::as_i64)
            .is_none_or(|v| v <= 0)
        || budget
            .get("policy_digest")
            .and_then(Value::as_str)
            .is_none_or(|v| {
                v.len() != 64
                    || !v
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
    {
        return Err(Error::InvalidArguments);
    }
    // The application/Pg continuation validates signature and exact reservation binding.
    let expected = json!({"provider_profile_ref":{"id":prepared.profile()},"model_configuration":{"model":prepared.model()},"adapter_version":"1","budget_policy_id":policy_id,"budget_policy":budget,"destination":prepared.destination(),"wire_version":prepared.wire_version(),"request_body_length":prepared.body_length(),"request_body_sha256":prepared.body_sha256()});
    if saved.opportunity.capability != AdvisoryCapability::ScopeDecomposition
        || saved.opportunity.decision_point
            != AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection
        || saved.opportunity.target_kind != "scope_candidate_set"
        || saved.opportunity.target_id.is_none()
        || saved.dispatch.id.is_nil()
        || saved.dispatch.opportunity_id != saved.opportunity.id
        || saved.dispatch.state != AdvisoryDispatchState::Sealed
        || saved.dispatch.send_certainty != AdvisorySendCertainty::Sent
        || saved.dispatch.outcome != Some(AdvisoryDispatchOutcome::ProviderResponse)
        || saved.dispatch.provider != "jev-system-one"
        || saved.dispatch.model != provider.config.model
        || saved.dispatch.material_digest != saved.opportunity.material_digest
        || prepared.profile() != provider.config.profile
        || prepared.model() != provider.config.model
        || prepared.destination() != provider.config.endpoint.as_str()
        || prepared.wire_version() != WIRE_FORMAT
        || saved.request_payload != prepared.body()
        || saved.request_payload_sha256 != prepared.body_sha256()
        || saved.dispatch.payload_digest != prepared.body_sha256()
        || prepared.body_sha256() != format!("{:x}", Sha256::digest(prepared.body()))
        || prepared.body_length() != prepared.body().len()
        || prepared.body_length() > provider.config.maximum_request_bytes
        || snapshot != &expected
        || saved.dispatch.configuration_digest
            != format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(snapshot).map_err(|_| Error::InvalidArguments)?)
            )
        || !raw.response_complete
        || !raw
            .http_status
            .is_some_and(|status| (200..300).contains(&status))
        || context.send_certainty != AdvisorySendCertainty::Sent
        || context.outcome != AdvisoryDispatchOutcome::ProviderResponse
        || context.provider_failure_code.is_some()
        || bytes.len() > provider.config.maximum_response_bytes
    {
        return Err(Error::InputConflict);
    }
    let body: Value =
        serde_json::from_slice(prepared.body()).map_err(|_| Error::InvalidArguments)?;
    let emitted: Vec<ScopeDecompositionAlternative> = serde_json::from_value(
        body.get("state")
            .and_then(|v| v.get("emitted"))
            .cloned()
            .ok_or(Error::InvalidArguments)?,
    )
    .map_err(|_| Error::InvalidArguments)?;
    if emitted.is_empty() {
        return Err(Error::InvalidArguments);
    }
    let canonical = wire::serialize_request(prepared.model(), prepared.request(), &emitted)
        .map_err(|_| Error::InvalidArguments)?;
    if canonical != prepared.body() {
        return Err(Error::InputConflict);
    }
    wire::parse_response(bytes, prepared.model(), prepared.request())
        .map(|v| v.answers)
        .map_err(|_| Error::InvalidArguments)
}
