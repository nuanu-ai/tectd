//! Three explicit transactions surround at most one optional adviser call.
//! The caller commits the start before transport and the raw seal before parse.
use crate::{
    ModelRouteAttemptStore, ModelRouteInvocation, ModelRoutePreparation, ModelRoutePreparedAttempt,
    ModelRouteProviderObservation, ModelRouteRankingProvider, ModelRouteRunNoCall,
    ModelRouteSealedRankingEvidence, ModelRouteSendPermit, PreparedModelRouteRecommendation,
};
use std::future::Future;
use tect_domain::{Error, ModelRouteRankingWireOutcome, Result, model_route_wire_sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelRouteSendStart {
    NoCall(ModelRouteRunNoCall),
    Started {
        attempted: Box<ModelRoutePreparedAttempt>,
        permit: ModelRouteSendPermit,
    },
    Replay,
}

pub async fn prepare_model_route_send(
    store: &mut dyn ModelRouteAttemptStore,
    provider: &dyn ModelRouteRankingProvider,
    prepared: &PreparedModelRouteRecommendation,
    invocation: ModelRouteInvocation,
) -> Result<ModelRouteSendStart> {
    let reason = if prepared.preparation != ModelRoutePreparation::Prepared {
        Some(ModelRouteRunNoCall::Preparation(prepared.preparation))
    } else if !provider.available() {
        Some(ModelRouteRunNoCall::ProviderUnavailable)
    } else if let Some(profile) = provider.required_profile() {
        if !store.provider_profile_matches(prepared, profile).await? {
            Some(ModelRouteRunNoCall::ProviderUnavailable)
        } else {
            None
        }
    } else {
        None
    };
    if let Some(reason) = reason {
        store.record_no_call(prepared, invocation, reason).await?;
        return Ok(ModelRouteSendStart::NoCall(reason));
    }
    let attempted = match provider.prepare(prepared) {
        Ok(value) => value,
        Err(_) => {
            store
                .record_no_call(
                    prepared,
                    invocation,
                    ModelRouteRunNoCall::ProviderUnavailable,
                )
                .await?;
            return Ok(ModelRouteSendStart::NoCall(
                ModelRouteRunNoCall::ProviderUnavailable,
            ));
        }
    };
    if attempted.verify(prepared).is_err()
        || (attempted.adapter_identity.is_some() && provider.required_profile().is_none())
    {
        store
            .record_no_call(
                prepared,
                invocation,
                ModelRouteRunNoCall::ProviderUnavailable,
            )
            .await?;
        return Ok(ModelRouteSendStart::NoCall(
            ModelRouteRunNoCall::ProviderUnavailable,
        ));
    }
    if let Some(existing) = store
        .by_preparation(prepared.workspace_id, &prepared.request_key, invocation)
        .await?
    {
        return if existing.request_sha256.as_deref() == Some(attempted.request_sha256.as_str()) {
            Ok(ModelRouteSendStart::Replay)
        } else {
            Err(Error::InputConflict)
        };
    }
    let now_unix_ms = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::BudgetPolicyInvalid)?
            .as_millis(),
    )
    .map_err(|_| Error::BudgetPolicyInvalid)?;
    let policy = store
        .authorized_budget_policy(prepared.workspace_id, now_unix_ms)
        .await?
        .ok_or(Error::BudgetPolicyInvalid)?;
    policy.validate().map_err(|_| Error::BudgetPolicyInvalid)?;
    if !policy.is_effective_at(now_unix_ms) {
        return Err(Error::BudgetPolicyInvalid);
    }
    match store
        .begin_send(
            prepared,
            invocation,
            &attempted,
            &policy,
            provider.required_profile(),
        )
        .await?
    {
        Some(permit)
            if permit.workspace_id == prepared.workspace_id
                && permit.preparation_request_key == prepared.request_key
                && permit.request_sha256 == attempted.request_sha256
                && permit.policy_id == policy.id()
                && permit.policy_version == policy.version()
                && permit.policy_digest == policy.digest()
                && !permit.attempt_id.is_nil() =>
        {
            Ok(ModelRouteSendStart::Started {
                attempted: Box::new(attempted),
                permit,
            })
        }
        None => Ok(ModelRouteSendStart::Replay),
        _ => Err(Error::InputConflict),
    }
}

pub async fn attempt_model_route_observed_after_commit<F: Future<Output = Result<()>>>(
    commit: F,
    provider: &dyn ModelRouteRankingProvider,
    attempted: ModelRoutePreparedAttempt,
    permit: ModelRouteSendPermit,
) -> Result<Result<ModelRouteProviderObservation>> {
    commit.await?;
    let start = std::time::Instant::now();
    let observed = provider
        .attempt_prepared_observed(attempted, permit)
        .await
        .map(|mut observation| {
            observation.elapsed_monotonic_ms = i64::try_from(start.elapsed().as_millis()).ok();
            observation
        });
    Ok(observed)
}

/// The Future must commit the one-use start transaction. A failed commit
/// precludes transport; an uncertain provider error must be marked separately.
pub async fn attempt_model_route_after_commit<F: Future<Output = Result<()>>>(
    commit: F,
    provider: &dyn ModelRouteRankingProvider,
    attempted: ModelRoutePreparedAttempt,
    permit: ModelRouteSendPermit,
) -> Result<Result<Vec<u8>>> {
    commit.await?;
    Ok(provider.attempt_prepared(attempted, permit).await)
}

pub async fn seal_model_route_raw_response(
    store: &mut dyn ModelRouteAttemptStore,
    permit: &ModelRouteSendPermit,
    raw: &[u8],
) -> Result<String> {
    let digest = model_route_wire_sha256(raw);
    store.seal_raw_response(permit, raw, &digest).await?;
    Ok(digest)
}

/// Call only after the raw-seal transaction commits. Malformed bytes remain
/// sealed and cannot be replaced or retried as another provider attempt.
pub async fn finalize_model_route_sealed_response(
    store: &mut dyn ModelRouteAttemptStore,
    prepared: &PreparedModelRouteRecommendation,
    attempted: &ModelRoutePreparedAttempt,
    permit: &ModelRouteSendPermit,
) -> Result<ModelRouteRankingWireOutcome> {
    finalize_model_route_provider_response(
        store,
        &crate::DisabledModelRouteRankingProvider,
        prepared,
        attempted,
        permit,
    )
    .await
}

pub async fn finalize_model_route_provider_response(
    store: &mut dyn ModelRouteAttemptStore,
    provider: &dyn ModelRouteRankingProvider,
    prepared: &PreparedModelRouteRecommendation,
    attempted: &ModelRoutePreparedAttempt,
    permit: &ModelRouteSendPermit,
) -> Result<ModelRouteRankingWireOutcome> {
    attempted.verify(prepared)?;
    let observation = store
        .sealed_observation(permit)
        .await?
        .ok_or(Error::StaleContext)?;
    if store.consumption_healthy(permit).await? != Some(true) {
        return Err(Error::BudgetPolicyInvalid);
    }
    if observation.response_complete == Some(false)
        || (attempted.adapter_identity.is_some() && observation.response_complete != Some(true))
        || observation
            .http_status
            .is_some_and(|s| !(200..300).contains(&s))
    {
        return Err(Error::InvalidArguments);
    }
    let outcome = provider.parse_sealed(attempted, &observation)?;
    let raw = observation.raw;
    let evidence = ModelRouteSealedRankingEvidence {
        permit: permit.clone(),
        attempted: attempted.clone(),
        response_sha256: model_route_wire_sha256(&raw),
        raw_response: raw,
        outcome: outcome.clone(),
    };
    evidence.validate_material(prepared)?;
    store.capture_provider_outcome(&evidence, provider).await?;
    Ok(outcome)
}

/// Pure usage extraction after the caller has loaded the committed raw seal.
pub(crate) fn observe_model_route_sealed_usage(
    provider: &dyn ModelRouteRankingProvider,
    attempted: &ModelRoutePreparedAttempt,
    observation: &mut ModelRouteProviderObservation,
) -> bool {
    let usage = if observation.response_complete == Some(false)
        || (attempted.adapter_identity.is_some() && observation.response_complete != Some(true))
    {
        Err(Error::InvalidArguments)
    } else {
        provider.sealed_usage(attempted, observation)
    };
    let invalid = usage.is_err();
    let usage = usage.unwrap_or(crate::ModelRouteUsage {
        input_tokens: None,
        output_tokens: None,
    });
    observation.input_tokens = usage.input_tokens;
    observation.output_tokens = usage.output_tokens;
    invalid
}

#[cfg(test)]
mod tests;
