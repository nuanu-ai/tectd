use crate::{
    AdvisoryLifecycleCapability, GuardedMatrixAdviceRecord, MatrixBudgetAuthorization,
    MatrixBudgetRequest, MatrixProviderIdentity, MatrixProviderRequest,
    PreparedMatrixAdviceAttempt, StoredMatrixDispatch, TransactionMode, WorkspaceService,
};
use sha2::{Digest, Sha256};
#[cfg(test)]
use tect_domain::AdvisoryDispatchSeal;
use tect_domain::{
    AdvisoryBudgetPolicy, AdvisoryDispatchAuthorization, AdvisoryDispatchOutcome,
    AdvisoryDispatchState, AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryReason,
    AdvisoryRetryBasis, AdvisorySendCertainty, Error, RequestContext, Result,
    WorkspaceAdvisoryMode,
};
use uuid::Uuid;

pub(crate) struct PreparedMatrixDispatch {
    pub opportunity: AdvisoryOpportunity,
    pub config_revision: i64,
    pub authorization: AdvisoryDispatchAuthorization,
    pub provider_request: MatrixProviderRequest,
    pub prepared: PreparedMatrixAdviceAttempt,
}

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
    opportunity_id: Uuid,
    opportunity: &AdvisoryOpportunity,
    prepared: &PreparedMatrixAdviceAttempt,
    budget: &MatrixBudgetAuthorization,
    verified_policy: &AdvisoryBudgetPolicy,
) -> Result<AdvisoryDispatchAuthorization> {
    if budget.policy_id != verified_policy.id().to_string() {
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
    });
    let configuration_digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&configuration_snapshot).map_err(|_| Error::InternalInvariant)?
        )
    );
    let authorization = AdvisoryDispatchAuthorization {
        dispatch_id: Uuid::new_v4(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatrixRecoveryWindow {
    Authorized,
    SealedResponse,
    ReceiptOnly,
}

fn matrix_recovery_window(
    opportunity: AdvisoryOpportunityState,
    dispatch: AdvisoryDispatchState,
    certainty: AdvisorySendCertainty,
    outcome: Option<AdvisoryDispatchOutcome>,
) -> MatrixRecoveryWindow {
    match (opportunity, dispatch, certainty, outcome) {
        (
            AdvisoryOpportunityState::Prepared,
            AdvisoryDispatchState::Authorized,
            AdvisorySendCertainty::NotSent,
            None,
        ) => MatrixRecoveryWindow::Authorized,
        (
            AdvisoryOpportunityState::AwaitingResponse,
            AdvisoryDispatchState::Sealed,
            AdvisorySendCertainty::Sent,
            Some(AdvisoryDispatchOutcome::ProviderResponse),
        ) => MatrixRecoveryWindow::SealedResponse,
        _ => MatrixRecoveryWindow::ReceiptOnly,
    }
}

mod recovery;

fn matrix_observation_allows_parse(saved: &StoredMatrixDispatch) -> bool {
    saved.response_complete
        && saved.response_payload.is_some()
        && match saved.response_http_status {
            Some(status) => (200..=299).contains(&status),
            None => saved.wire_version != "tect.matrix-typesafe-native/1",
        }
}

impl WorkspaceService {
    async fn matrix_request_is_current(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        provider_request: &MatrixProviderRequest,
        expected_config_revision: i64,
    ) -> Result<bool> {
        // External evidence validation runs without holding write locks. The
        // write transaction rechecks database task and verification bindings
        // under config-then-task locks before sending or recording advice.
        let (mut read, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let session = read
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        if Self::validate_binding(&mut *read, context, &identity, &session)
            .await?
            .id
            != workspace_id
        {
            return Err(Error::InputConflict);
        }
        let current = read
            .matrix_task(workspace_id, provider_request.revision().task_id)
            .await?;
        let config = read.advisory_config(workspace_id).await?;
        let configured = config.revision == expected_config_revision
            && config.mode == WorkspaceAdvisoryMode::Optional
            && config.provider_profile_ref.as_ref()
                == Some(provider_request.provider_profile_ref())
            && config.model_configuration.as_ref() == Some(provider_request.model_configuration());
        let fresh = crate::matrix_tasks::matrix_request_still_current(
            read.matrix_verification_store(),
            self.matrix_evidence_validator.as_ref(),
            workspace_id,
            current,
            provider_request,
        )
        .await;
        read.commit().await?;
        Ok(fresh && configured)
    }

    pub(crate) async fn dispatch_prepared_matrix_advisory(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        dispatch: PreparedMatrixDispatch,
    ) -> Result<AdvisoryOpportunity> {
        let PreparedMatrixDispatch {
            opportunity,
            config_revision,
            authorization,
            provider_request,
            prepared,
        } = dispatch;
        let lifecycle = AdvisoryLifecycleCapability::internal();
        let verification_current = self
            .matrix_request_is_current(context, workspace_id, &provider_request, config_revision)
            .await?
            && prepared.validate_for(&provider_request).is_ok()
            && authorization.payload_digest == prepared.body_sha256()
            && authorization.request_payload == prepared.body();
        let (mut start, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        let session = start
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        if Self::validate_binding(&mut *start, context, &identity, &session)
            .await?
            .id
            != workspace_id
        {
            return Err(Error::InputConflict);
        }
        let started = start
            .start_verified_matrix_dispatch(
                &lifecycle,
                workspace_id,
                authorization.dispatch_id,
                verification_current,
            )
            .await?;
        start.commit().await?;
        if !started.should_send {
            let (mut read, identity) = self
                .authenticated(context, TransactionMode::ReadWrite)
                .await?;
            let session = read
                .session(identity.host_id, &context.native_session_id)
                .await?
                .ok_or(Error::WorkspaceNotOpen)?;
            if Self::validate_binding(&mut *read, context, &identity, &session)
                .await?
                .id
                != workspace_id
            {
                return Err(Error::InputConflict);
            }
            let terminal = read
                .advisory_opportunity_for_dispatch(workspace_id, opportunity.id)
                .await?;
            read.commit().await?;
            return Ok(terminal);
        }
        let permit = crate::MatrixStartedDispatchPermit::after_committed_start(
            &started,
            &authorization,
            &opportunity,
            &provider_request,
            &prepared,
        )?;
        let continuation = crate::MatrixDispatchContinuation::from_started(
            &permit,
            crate::AdvisoryDispatchContinuation::after_committed_start(
                identity.tenant_id,
                workspace_id,
                &opportunity,
                &started,
                &authorization,
            )?,
        )?;
        // The committed Sending row is the one-use boundary. A transport error
        // remains uncertain and is never retried by this request or its replay.
        let monotonic_start = std::time::Instant::now();
        let observed = self
            .matrix_advice_provider
            .observe_prepared(prepared, permit)
            .await
            .unwrap_or(crate::MatrixProviderObservation {
                response_payload: None,
                http_status: None,
                input_tokens: None,
                output_tokens: None,
                legacy_response: None,
                response_complete: false,
                original_transport_context: Some(crate::AdvisoryProviderTransportContext {
                    send_certainty: AdvisorySendCertainty::SentUnknown,
                    outcome: AdvisoryDispatchOutcome::ProviderFailure,
                    raw_response_ref: None,
                    provider_failure_code: Some("transport-unknown".into()),
                }),
            });
        let monotonic_elapsed_ms =
            i64::try_from(monotonic_start.elapsed().as_millis()).unwrap_or(i64::MAX);
        let saved = self
            .seal_committed_matrix_observation(
                identity.tenant_id,
                &continuation,
                &observed,
                monotonic_elapsed_ms,
            )
            .await?;
        let usage = if saved.response_complete {
            self.matrix_advice_provider.sealed_response_usage(&saved)
        } else {
            crate::MatrixProviderUsage::default()
        };
        let (saved, consumption) = self
            .consume_committed_matrix_observation(identity.tenant_id, &continuation, usage)
            .await?;
        let dispatch = &saved.dispatch;
        let verification_stale = !self
            .matrix_request_is_current(context, workspace_id, &provider_request, config_revision)
            .await?;
        let guarded = if !consumption.exhausted_after_response
            && !verification_stale
            && matrix_observation_allows_parse(&saved)
        {
            observed
                .legacy_response
                .map(Ok)
                .unwrap_or_else(|| {
                    self.matrix_advice_provider
                        .parse_sealed_response(&provider_request, &saved)
                })
                .and_then(|response| {
                    GuardedMatrixAdviceRecord::from_provider_response(
                        opportunity.id,
                        dispatch.id,
                        &provider_request,
                        response,
                    )
                })
                .ok()
        } else {
            None
        };
        let (mut finalize, _) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        let result = finalize
            .finalize_guarded_matrix_advice(
                &lifecycle,
                workspace_id,
                opportunity.id,
                config_revision,
                dispatch,
                if consumption.exhausted_after_response {
                    None
                } else {
                    guarded.as_ref()
                },
                verification_stale,
            )
            .await?;
        finalize.commit().await?;
        Ok(result)
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn only_unsent_authorized_attempt_can_enter_transport_window() {
        assert_eq!(
            matrix_recovery_window(
                AdvisoryOpportunityState::Prepared,
                AdvisoryDispatchState::Authorized,
                AdvisorySendCertainty::NotSent,
                None,
            ),
            MatrixRecoveryWindow::Authorized,
        );
        for (state, certainty, outcome) in [
            (
                AdvisoryDispatchState::Sending,
                AdvisorySendCertainty::SentUnknown,
                None,
            ),
            (
                AdvisoryDispatchState::Cancelled,
                AdvisorySendCertainty::NotSent,
                None,
            ),
            (
                AdvisoryDispatchState::Sealed,
                AdvisorySendCertainty::SentUnknown,
                Some(AdvisoryDispatchOutcome::ProviderFailure),
            ),
            (
                AdvisoryDispatchState::Sealed,
                AdvisorySendCertainty::Sent,
                Some(AdvisoryDispatchOutcome::ProviderResponse),
            ),
        ] {
            assert_eq!(
                matrix_recovery_window(
                    AdvisoryOpportunityState::Prepared,
                    state,
                    certainty,
                    outcome
                ),
                MatrixRecoveryWindow::ReceiptOnly,
            );
        }
    }

    #[test]
    fn only_sealed_saved_response_can_enter_parse_window() {
        assert_eq!(
            matrix_recovery_window(
                AdvisoryOpportunityState::AwaitingResponse,
                AdvisoryDispatchState::Sealed,
                AdvisorySendCertainty::Sent,
                Some(AdvisoryDispatchOutcome::ProviderResponse),
            ),
            MatrixRecoveryWindow::SealedResponse,
        );
        for (state, certainty, outcome) in [
            (
                AdvisoryDispatchState::Sending,
                AdvisorySendCertainty::SentUnknown,
                None,
            ),
            (
                AdvisoryDispatchState::Sealed,
                AdvisorySendCertainty::SentUnknown,
                Some(AdvisoryDispatchOutcome::ProviderFailure),
            ),
            (
                AdvisoryDispatchState::Cancelled,
                AdvisorySendCertainty::NotSent,
                None,
            ),
        ] {
            assert_eq!(
                matrix_recovery_window(
                    AdvisoryOpportunityState::AwaitingResponse,
                    state,
                    certainty,
                    outcome
                ),
                MatrixRecoveryWindow::ReceiptOnly,
            );
        }
    }
}
