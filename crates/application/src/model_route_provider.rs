//! Three explicit transactions surround at most one optional adviser call.
//! The caller commits the start before transport and the raw seal before parse.
use crate::{
    ModelRouteAttemptStore, ModelRouteInvocation, ModelRoutePreparation, ModelRoutePreparedAttempt,
    ModelRouteRankingProvider, ModelRouteRunNoCall, ModelRouteSealedRankingEvidence,
    ModelRouteSendPermit, PreparedModelRouteRecommendation,
};
use std::future::Future;
use tect_domain::{
    Error, ModelRouteRankingWireOutcome, Result, model_route_wire_sha256,
    parse_model_route_ranking_response,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelRouteSendStart {
    NoCall(ModelRouteRunNoCall),
    Started {
        attempted: ModelRoutePreparedAttempt,
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
    } else {
        None
    };
    if let Some(reason) = reason {
        store.record_no_call(prepared, invocation, reason).await?;
        return Ok(ModelRouteSendStart::NoCall(reason));
    }
    let attempted = provider.prepare(prepared)?;
    attempted.verify(prepared)?;
    match store.begin_send(prepared, invocation, &attempted).await? {
        Some(permit)
            if permit.workspace_id == prepared.workspace_id
                && permit.preparation_request_key == prepared.request_key
                && permit.request_sha256 == attempted.request_sha256
                && !permit.attempt_id.is_nil() =>
        {
            Ok(ModelRouteSendStart::Started { attempted, permit })
        }
        None => Ok(ModelRouteSendStart::Replay),
        _ => Err(Error::InputConflict),
    }
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
    attempted.verify(prepared)?;
    let raw = store
        .sealed_response(permit)
        .await?
        .ok_or(Error::StaleContext)?;
    let outcome = parse_model_route_ranking_response(&attempted.request, &raw)?;
    let evidence = ModelRouteSealedRankingEvidence {
        permit: permit.clone(),
        attempted: attempted.clone(),
        response_sha256: model_route_wire_sha256(&raw),
        raw_response: raw,
        outcome: outcome.clone(),
    };
    evidence.verify(prepared)?;
    store.capture_sealed_outcome(&evidence).await?;
    Ok(outcome)
}

#[cfg(test)]
mod tests;
