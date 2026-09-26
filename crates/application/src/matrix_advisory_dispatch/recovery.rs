use super::*;

impl WorkspaceService {
    async fn current_verified_matrix_budget(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
    ) -> Result<Option<tect_domain::AdvisoryBudgetPolicy>> {
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
        let policy = crate::matrix_advisory_capture::lookup_verified_matrix_budget(
            read.advisory_budget_policy_store(),
            workspace_id,
        )
        .await?;
        read.commit().await?;
        Ok(policy)
    }

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
        mut saved: StoredMatrixDispatch,
    ) -> Result<AdvisoryOpportunity> {
        let mut exhausted = false;
        if saved.raw_observation_sealed
            && opportunity.state == AdvisoryOpportunityState::AwaitingResponse
        {
            let (read, identity) = self
                .authenticated(context, TransactionMode::ReadOnly)
                .await?;
            read.commit().await?;
            let continuation = crate::MatrixDispatchContinuation::from_saved(
                &saved,
                workspace_id,
                opportunity.authorized_actor_id,
            );
            let usage = if saved.response_complete {
                self.matrix_advice_provider.sealed_response_usage(&saved)
            } else {
                crate::MatrixProviderUsage::default()
            };
            let (consumed, consumption) = self
                .consume_committed_matrix_observation(identity.tenant_id, &continuation, usage)
                .await?;
            saved = consumed;
            exhausted = consumption.exhausted_after_response;
            if saved.dispatch.send_certainty == AdvisorySendCertainty::SentUnknown {
                let (mut finalize, _) = self
                    .authenticated(context, TransactionMode::ReadWrite)
                    .await?;
                let result = finalize
                    .finalize_guarded_matrix_advice(
                        &AdvisoryLifecycleCapability::internal(),
                        workspace_id,
                        opportunity.id,
                        opportunity.config_revision,
                        &saved.dispatch,
                        None,
                        false,
                    )
                    .await?;
                finalize.commit().await?;
                return Ok(result);
            }
        }
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
                let Some(verified_policy) = self
                    .current_verified_matrix_budget(context, workspace_id)
                    .await?
                else {
                    return self
                        .cancel_stale_authorized_matrix_dispatch(
                            context,
                            workspace_id,
                            opportunity.id,
                            dispatch.id,
                        )
                        .await;
                };
                if self
                    .matrix_budget
                    .authorize(&budget_request, &verified_policy)
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
                    &verified_policy,
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
                    super::PreparedMatrixDispatch {
                        opportunity: opportunity.clone(),
                        config_revision: opportunity.config_revision,
                        authorization,
                        provider_request: request,
                        prepared,
                    },
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
                    self.matrix_request_is_current(
                        context,
                        workspace_id,
                        request,
                        opportunity.config_revision,
                    )
                    .await?
                } else {
                    false
                };
                let lifecycle = AdvisoryLifecycleCapability::internal();
                if !saved.raw_observation_sealed {
                    let (mut consume, _) = self
                        .authenticated(context, TransactionMode::ReadWrite)
                        .await?;
                    exhausted = consume
                        .consume_advisory_budget(&lifecycle, workspace_id, dispatch.id)
                        .await?
                        .exhausted_after_response;
                    consume.commit().await?;
                }
                let guarded =
                    if current && !exhausted && super::matrix_observation_allows_parse(&saved) {
                        let request = request.as_ref().ok_or(Error::StaleContext)?;
                        let response = self
                            .matrix_advice_provider
                            .parse_sealed_response(request, &saved);
                        let Ok(response) = response else {
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
                                    None,
                                    !current,
                                )
                                .await?;
                            finalize.commit().await?;
                            return Ok(result);
                        };
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
}
