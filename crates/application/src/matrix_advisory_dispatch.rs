use crate::{
    AdvisoryLifecycleCapability, GuardedMatrixAdviceRecord, MatrixBudgetAuthorization,
    MatrixBudgetRequest, MatrixProviderIdentity, MatrixProviderRequest,
    PreparedMatrixAdviceAttempt, StoredMatrixDispatch, TransactionMode, WorkspaceService,
};
use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryDispatchAuthorization, AdvisoryDispatchOutcome, AdvisoryDispatchSeal,
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
) -> Result<AdvisoryDispatchAuthorization> {
    let identity = prepared.identity();
    let configuration_snapshot = serde_json::json!({
        "provider_profile_ref": identity.provider_profile_ref,
        "model_configuration": identity.model_configuration,
        "destination": identity.destination,
        "wire_version": identity.wire_version,
        "budget_policy_id": budget.policy_id,
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

impl WorkspaceService {
    async fn matrix_request_is_current(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        provider_request: &MatrixProviderRequest,
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
        let fresh = crate::matrix_tasks::matrix_request_still_current(
            read.matrix_verification_store(),
            self.matrix_evidence_validator.as_ref(),
            workspace_id,
            current,
            provider_request,
        )
        .await;
        read.commit().await?;
        Ok(fresh)
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
            .matrix_request_is_current(context, workspace_id, &provider_request)
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
        // The committed Sending row is the one-use boundary. A transport error
        // remains uncertain and is never retried by this request or its replay.
        let observed = self
            .matrix_advice_provider
            .attempt_prepared(prepared, permit)
            .await;
        let (seal, guarded) = seal_matrix_provider_observation(
            opportunity.id,
            authorization.dispatch_id,
            &provider_request,
            observed,
        );
        seal.validate()?;
        let (mut seal_tx, _) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        let dispatch = seal_tx
            .seal_advisory_dispatch(&lifecycle, workspace_id, &seal)
            .await?;
        seal_tx.commit().await?;
        let verification_stale = !self
            .matrix_request_is_current(context, workspace_id, &provider_request)
            .await?;
        let (mut finalize, _) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        let result = finalize
            .finalize_guarded_matrix_advice(
                &lifecycle,
                workspace_id,
                opportunity.id,
                config_revision,
                &dispatch,
                guarded.as_ref(),
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
