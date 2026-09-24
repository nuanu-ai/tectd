use crate::{
    AdvisoryLifecycleCapability, GuardedMatrixAdviceRecord, MatrixBudgetAuthorization,
    MatrixProviderRequest, PreparedMatrixAdviceAttempt, TransactionMode, WorkspaceService,
};
use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryDispatchAuthorization, AdvisoryDispatchOutcome, AdvisoryDispatchSeal,
    AdvisoryOpportunity, AdvisoryRetryBasis, AdvisorySendCertainty,
    Error, RequestContext, Result,
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

impl WorkspaceService {
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
            .start_advisory_dispatch(&lifecycle, workspace_id, authorization.dispatch_id)
            .await?;
        start.commit().await?;
        if !started.should_send {
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
            )
            .await?;
        finalize.commit().await?;
        Ok(result)
    }
}
