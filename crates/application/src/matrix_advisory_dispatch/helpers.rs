use super::*;

#[cfg(test)]
pub(crate) fn seal_matrix_provider_observation(
    opportunity_id: Uuid,
    dispatch_id: Uuid,
    request: &MatrixProviderRequest,
    observed: Result<crate::MatrixProviderResponse>,
) -> (AdvisoryDispatchSeal, Option<GuardedMatrixAdviceRecord>) {
    match observed {
        Ok(response) => {
            let raw = response.raw_response_payload.clone();
            let input_tokens = response
                .input_tokens
                .and_then(|value| i64::try_from(value).ok());
            let output_tokens = response
                .output_tokens
                .and_then(|value| i64::try_from(value).ok());
            let token_overflow = response
                .input_tokens
                .is_some_and(|value| i64::try_from(value).is_err())
                || response
                    .output_tokens
                    .is_some_and(|value| i64::try_from(value).is_err());
            let guarded = if token_overflow {
                None
            } else {
                GuardedMatrixAdviceRecord::from_provider_response(
                    opportunity_id,
                    dispatch_id,
                    request,
                    response,
                )
                .ok()
            };
            (
                AdvisoryDispatchSeal {
                    dispatch_id,
                    send_certainty: AdvisorySendCertainty::Sent,
                    outcome: if guarded.is_some() {
                        AdvisoryDispatchOutcome::ProviderResponse
                    } else {
                        AdvisoryDispatchOutcome::ProviderFailure
                    },
                    response_payload: Some(raw),
                    input_tokens,
                    output_tokens,
                    latency_ms: None,
                    raw_response_ref: None,
                },
                guarded,
            )
        }
        Err(_) => (
            AdvisoryDispatchSeal {
                dispatch_id,
                send_certainty: AdvisorySendCertainty::SentUnknown,
                outcome: AdvisoryDispatchOutcome::ProviderFailure,
                response_payload: None,
                input_tokens: None,
                output_tokens: None,
                latency_ms: None,
                raw_response_ref: None,
            },
            None,
        ),
    }
}

pub(crate) fn authorize_prepared_matrix(
    dispatch_id: Uuid,
    opportunity_id: Uuid,
    opportunity: &AdvisoryOpportunity,
    prepared: &PreparedMatrixAdviceAttempt,
    budget: &MatrixBudgetAuthorization,
    verified_policy: &AdvisoryBudgetPolicy,
) -> Result<AdvisoryDispatchAuthorization> {
    if dispatch_id.is_nil() || budget.policy_id != verified_policy.id().to_string() {
        return Err(Error::BudgetPolicyInvalid);
    }
    let identity = prepared.identity();
    let configuration_snapshot = serde_json::json!({
        "provider_profile_ref": identity.provider_profile_ref,
        "model_configuration": identity.model_configuration,
        "destination": identity.destination,
        "wire_version": identity.wire_version,
        "budget_policy_id": budget.policy_id,
        "budget_policy": {
            "policy_id": budget.policy_id,
            "policy_version": verified_policy.version(),
            "policy_digest": verified_policy.digest(),
        },
        "request_body_length": prepared.body_length(),
        "request_body_sha256": prepared.body_sha256(),
        "matrix_authority": match &prepared.binding().verification {
            crate::MatrixVerificationAuthority::ContextV2 { digest, snapshot_id, authority_schema, semantic_digest } => serde_json::json!({
                "schema": "tect.context-matrix-verification/1",
                "verification_digest": digest,
                "frozen_snapshot_id": snapshot_id,
                "authority_schema": authority_schema,
                "requirements_semantic_digest": semantic_digest,
            }),
            _ => return Err(Error::Forbidden),
        },
        "advisory_correlation": {
            "opportunity_id": opportunity_id,
            "dispatch_id": dispatch_id,
        },
    });
    let configuration_digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&configuration_snapshot).map_err(|_| Error::InternalInvariant)?
        )
    );
    let authorization = AdvisoryDispatchAuthorization {
        dispatch_id,
        opportunity_id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        retry_basis: AdvisoryRetryBasis::Initial,
        provider: identity.provider_profile_ref.id.clone(),
        model: identity.model_configuration.model.clone(),
        configuration_snapshot,
        configuration_digest,
        material_digest: opportunity.material_digest.clone(),
        payload_digest: prepared.body_sha256().into(),
        request_payload: prepared.body().to_vec(),
    };
    authorization.validate()?;
    Ok(authorization)
}
