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

impl WorkspaceService {
    async fn cancel_stale_authorized_matrix_dispatch(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryOpportunity> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        if Self::validate_binding(&mut *tx, context, &identity, &session)
            .await?
            .id
            != workspace_id
        {
            return Err(Error::InputConflict);
        }
        // The store rechecks the dispatch, configuration, and task under its
        // write locks; false can only cancel an Authorized, not-sent attempt.
        let started = tx
            .start_verified_matrix_dispatch(
                &AdvisoryLifecycleCapability::internal(),
                workspace_id,
                dispatch_id,
                false,
            )
            .await?;
        if started.should_send || started.dispatch.opportunity_id != opportunity_id {
            return Err(Error::InputConflict);
        }
        let terminal = tx
            .advisory_opportunity_for_dispatch(workspace_id, opportunity_id)
            .await?;
        tx.commit().await?;
        Ok(terminal)
    }

    async fn current_saved_matrix_request(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        saved: &StoredMatrixDispatch,
    ) -> Result<Option<MatrixProviderRequest>> {
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
            .matrix_task(workspace_id, saved.binding.task_id)
            .await?;
        let Some(current) =
            current.filter(|revision| revision.revision == saved.binding.task_revision)
        else {
            read.commit().await?;
            return Ok(None);
        };
        let verified = crate::matrix_tasks::compose_current_revision_with_validated_verification(
            read.matrix_verification_store(),
            self.matrix_evidence_validator.as_ref(),
            workspace_id,
            current.clone(),
            saved.binding.task_revision,
            crate::matrix_verification::current_epoch_seconds()?,
        )
        .await;
        read.commit().await?;
        let (composition, verification) = verified?;
        let Some(verification) = verification else {
            return Ok(None);
        };
        let Ok(request) = MatrixProviderRequest::new_verified(
            current,
            composition,
            &verification,
            saved.provider_profile_ref.clone(),
            saved.model_configuration.clone(),
        ) else {
            return Ok(None);
        };
        Ok((request.binding() == &saved.binding).then_some(request))
    }

    pub(crate) async fn recover_matrix_advisory(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        opportunity: AdvisoryOpportunity,
        saved: StoredMatrixDispatch,
    ) -> Result<AdvisoryOpportunity> {
        let dispatch = &saved.dispatch;
        if dispatch.opportunity_id != opportunity.id
            || dispatch.attempt_number != 1
            || dispatch.predecessor_dispatch_id.is_some()
            || dispatch.retry_basis != AdvisoryRetryBasis::Initial
            || dispatch.material_digest != opportunity.material_digest
            || dispatch.payload_digest != saved.request_payload_sha256
            || dispatch.configuration_digest
                != format!(
                    "{:x}",
                    Sha256::digest(
                        serde_json::to_vec(&saved.configuration_snapshot)
                            .map_err(|_| Error::InputConflict)?
                    )
                )
            || saved.binding.evaluation_digest != opportunity.material_digest
            || saved.binding.verification_digest != opportunity.matrix_verification_digest
            || opportunity.matrix_choice_set_digest.as_deref()
                != Some(&saved.binding.choice_set_digest)
            || opportunity.target_id != Some(saved.binding.task_id)
            || opportunity.matrix_task_revision != Some(saved.binding.task_revision)
            || opportunity.work_revision != Some(saved.binding.task_revision)
        {
            return Err(Error::InputConflict);
        }
        match matrix_recovery_window(
            opportunity.state,
            dispatch.state,
            dispatch.send_certainty,
            dispatch.outcome,
        ) {
            MatrixRecoveryWindow::Authorized => {
                if opportunity.primary_reason != AdvisoryReason::DispatchAuthorized
                    || opportunity.provider_called
                    || saved.response_payload.is_some()
                {
                    return Err(Error::InputConflict);
                }
                let request = self
                    .current_saved_matrix_request(context, workspace_id, &saved)
                    .await?;
                let Some(request) = request else {
                    return self
                        .cancel_stale_authorized_matrix_dispatch(
                            context,
                            workspace_id,
                            opportunity.id,
                            dispatch.id,
                        )
                        .await;
                };
                let (mut read, identity) = self
                    .authenticated(context, TransactionMode::ReadOnly)
                    .await?;
                let config = read.advisory_config(workspace_id).await?;
                read.commit().await?;
                if identity.principal_id != opportunity.authorized_actor_id {
                    return Err(Error::InputConflict);
                }
                if config.revision != opportunity.config_revision
                    || config.mode == WorkspaceAdvisoryMode::Disabled
                    || config.provider_profile_ref.as_ref() != Some(&saved.provider_profile_ref)
                    || config.model_configuration.as_ref() != Some(&saved.model_configuration)
                {
                    return self
                        .cancel_stale_authorized_matrix_dispatch(
                            context,
                            workspace_id,
                            opportunity.id,
                            dispatch.id,
                        )
                        .await;
                }
                let identity = MatrixProviderIdentity {
                    provider_profile_ref: saved.provider_profile_ref.clone(),
                    model_configuration: saved.model_configuration.clone(),
                    destination: saved.destination.clone(),
                    wire_version: saved.wire_version.clone(),
                };
                if self.matrix_advice_provider.identity().as_ref() != Some(&identity) {
                    return self
                        .cancel_stale_authorized_matrix_dispatch(
                            context,
                            workspace_id,
                            opportunity.id,
                            dispatch.id,
                        )
                        .await;
                }
                let prepared = PreparedMatrixAdviceAttempt::new(
                    &request,
                    identity,
                    saved.request_payload.clone(),
                )?;
                // Preparation and budget evaluation are pure ports. Both must still
                // agree with the original authorization before the one-use start.
                if self.matrix_advice_provider.prepare(&request)? != prepared {
                    return self
                        .cancel_stale_authorized_matrix_dispatch(
                            context,
                            workspace_id,
                            opportunity.id,
                            dispatch.id,
                        )
                        .await;
                }
                let policy_id = saved
                    .configuration_snapshot
                    .get("budget_policy_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or(Error::InputConflict)?;
                let budget_request = MatrixBudgetRequest::from_prepared(
                    workspace_id,
                    opportunity.authorized_actor_id,
                    &prepared,
                )?;
                if self
                    .matrix_budget
                    .authorize(&budget_request)
                    .await?
                    .as_ref()
                    != Some(&MatrixBudgetAuthorization {
                        policy_id: policy_id.to_owned(),
                    })
                {
                    return self
                        .cancel_stale_authorized_matrix_dispatch(
                            context,
                            workspace_id,
                            opportunity.id,
                            dispatch.id,
                        )
                        .await;
                }
                let expected = authorize_prepared_matrix(
                    opportunity.id,
                    &opportunity,
                    &prepared,
                    &MatrixBudgetAuthorization {
                        policy_id: policy_id.to_owned(),
                    },
                )?;
                if expected.configuration_snapshot != saved.configuration_snapshot
                    || expected.configuration_digest != dispatch.configuration_digest
                    || expected.material_digest != dispatch.material_digest
                    || expected.payload_digest != dispatch.payload_digest
                    || expected.request_payload != saved.request_payload
                {
                    return Err(Error::InputConflict);
                }
                let authorization = AdvisoryDispatchAuthorization {
                    dispatch_id: dispatch.id,
                    opportunity_id: opportunity.id,
                    predecessor_dispatch_id: None,
                    attempt_number: 1,
                    retry_basis: AdvisoryRetryBasis::Initial,
                    provider: dispatch.provider.clone(),
                    model: dispatch.model.clone(),
                    configuration_snapshot: saved.configuration_snapshot.clone(),
                    configuration_digest: dispatch.configuration_digest.clone(),
                    material_digest: dispatch.material_digest.clone(),
                    payload_digest: dispatch.payload_digest.clone(),
                    request_payload: saved.request_payload.clone(),
                };
                authorization.validate()?;
                if authorization.configuration_snapshot.get("budget_policy_id")
                    != Some(&serde_json::json!(policy_id))
                    || dispatch.provider != saved.provider_profile_ref.id
                    || dispatch.model != saved.model_configuration.model
                {
                    return Err(Error::InputConflict);
                }
                self.dispatch_prepared_matrix_advisory(
                    context,
                    workspace_id,
                    opportunity.clone(),
                    opportunity.config_revision,
                    authorization,
                    request,
                    prepared,
                )
                .await
            }
            MatrixRecoveryWindow::SealedResponse => {
                if saved.response_payload.is_none() {
                    return Err(Error::InputConflict);
                }
                let request = self
                    .current_saved_matrix_request(context, workspace_id, &saved)
                    .await?;
                let current = if let Some(request) = request.as_ref() {
                    self.matrix_request_is_current(context, workspace_id, request)
                        .await?
                } else {
                    false
                };
                let guarded = if current {
                    let request = request.as_ref().ok_or(Error::StaleContext)?;
                    let response = self
                        .matrix_advice_provider
                        .parse_sealed_response(request, &saved)?;
                    response.validate_for(request)?;
                    if saved.response_payload.as_deref()
                        != Some(response.raw_response_payload.as_slice())
                        || saved.response_payload_sha256.as_deref()
                            != Some(response.response_payload_sha256.as_str())
                        || dispatch.input_tokens
                            != response
                                .input_tokens
                                .map(i64::try_from)
                                .transpose()
                                .map_err(|_| Error::InputConflict)?
                        || dispatch.output_tokens
                            != response
                                .output_tokens
                                .map(i64::try_from)
                                .transpose()
                                .map_err(|_| Error::InputConflict)?
                    {
                        return Err(Error::InputConflict);
                    }
                    Some(GuardedMatrixAdviceRecord::from_provider_response(
                        opportunity.id,
                        dispatch.id,
                        request,
                        response,
                    )?)
                } else {
                    None
                };
                let lifecycle = AdvisoryLifecycleCapability::internal();
                let (mut finalize, _) = self
                    .authenticated(context, TransactionMode::ReadWrite)
                    .await?;
                let result = finalize
                    .finalize_guarded_matrix_advice(
                        &lifecycle,
                        workspace_id,
                        opportunity.id,
                        opportunity.config_revision,
                        dispatch,
                        guarded.as_ref(),
                        !current,
                    )
                    .await?;
                finalize.commit().await?;
                Ok(result)
            }
            MatrixRecoveryWindow::ReceiptOnly => Ok(opportunity),
        }
    }

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
        opportunity: AdvisoryOpportunity,
        config_revision: i64,
        authorization: AdvisoryDispatchAuthorization,
        provider_request: MatrixProviderRequest,
        prepared: PreparedMatrixAdviceAttempt,
    ) -> Result<AdvisoryOpportunity> {
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
