use super::prepare::PreparedScopeContext;
use super::*;

impl WorkspaceService {
    pub(super) async fn dispatch_scope_advisory(
        &self,
        context: &RequestContext,
        request: &RunScopeAdvisory,
        prepared: PreparedScopeContext,
    ) -> Result<ScopeAdvisoryOutcome> {
        let PreparedScopeContext {
            run:
                RunScopeContext {
                    identity,
                    workspace,
                    config,
                    manifest,
                    ..
                },
            opportunity,
            prepared_attempt,
            typed_request,
            policy,
            verified_policy,
        } = prepared;
        let dispatch_id = Uuid::new_v4();
        let (provider_name, adapter_version) = self
            .scope_advice_provider
            .identity()
            .ok_or(Error::TransportUnavailable)?;
        let authorization = scope_dispatch_authorization(
            &prepared_attempt,
            ScopeDispatchMetadata {
                dispatch_id,
                opportunity_id: opportunity.id,
                provider: provider_name,
                adapter_version,
                material_digest: opportunity.material_digest.clone(),
            },
            &config,
            &policy,
            &verified_policy,
        )?;
        let lifecycle = AdvisoryLifecycleCapability::internal();
        let (mut authorize, authorize_workspace, authorize_session) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        if authorize_workspace.id != workspace.id
            || authorize_session.id != opportunity.session_id
            || opportunity.session_preference != AdvisoryRequestPreference::UseWorkspace
        {
            return Err(Error::InputConflict);
        }
        // Same native-session lock as preference.set, held through commit.
        if authorize
            .session_advisory_preference(workspace.id, authorize_session.id)
            .await?
            .preference
            == AdvisoryRequestPreference::Skip
            && authorize.advisory_config(workspace.id).await? == config
        {
            let disposition = crate::ScopePreparedAdvisoryDisposition {
                opportunity_id: opportunity.id,
                candidate_set_id: request.candidate_set_id,
                expected_source_digest: manifest.source.digest.clone(),
                reason: AdvisoryReason::SessionSkip,
            };
            authorize
                .finalize_prepared_scope_advisory_without_dispatch(workspace.id, &disposition)
                .await?;
            let terminal = authorize
                .advisory_opportunity_for_dispatch(workspace.id, opportunity.id)
                .await?;
            let terminal = validate_terminalized_pre_dispatch_opportunity(terminal)?;
            authorize.commit().await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity: terminal,
                advice: None,
            });
        }
        let authorized = match authorize
            .authorize_advisory_dispatch(&lifecycle, workspace.id, config.revision, &authorization)
            .await
        {
            Ok(value) => value,
            Err(error) => {
                if let Some(reason) = prepared_scope_stale_reason(&error) {
                    // The persistence adapter returns these two stale codes only
                    // before creating a dispatch row. Drop this failed UOW so
                    // its transaction rolls back, then CAS the prepared record
                    // to an audited no-call/invalidated state in a fresh UOW.
                    drop(authorize);
                    return self
                        .finalize_prepared_scope_stale(
                            context,
                            workspace.id,
                            opportunity.id,
                            request.candidate_set_id,
                            &manifest.source.digest,
                            reason,
                        )
                        .await;
                }
                return Err(error);
            }
        };
        authorize.commit().await?;
        let (mut start, _, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let started = start
            .start_signed_scope_dispatch(&lifecycle, workspace.id, authorized.id)
            .await?;
        start.commit().await?;
        if !started.should_send {
            if started.dispatch.state == AdvisoryDispatchState::Cancelled
                && started.dispatch.send_certainty == AdvisorySendCertainty::NotSent
                && started.dispatch.opportunity_id == opportunity.id
                && started.dispatch.outcome.is_none()
                && started.dispatch.input_tokens.is_none()
                && started.dispatch.output_tokens.is_none()
                && started.dispatch.latency_ms.is_none()
                && started.dispatch.raw_response_ref.is_none()
            {
                let (mut terminal_tx, terminal_workspace, _) = self
                    .scope_transaction(context, TransactionMode::ReadOnly)
                    .await?;
                if terminal_workspace.id != workspace.id {
                    return Err(Error::InputConflict);
                }
                let terminal = terminal_tx
                    .advisory_opportunity_for_dispatch(workspace.id, opportunity.id)
                    .await?;
                let terminal = validate_terminalized_pre_dispatch_opportunity(terminal)?;
                terminal_tx.commit().await?;
                return Ok(ScopeAdvisoryOutcome {
                    opportunity: terminal,
                    advice: None,
                });
            }
            return Err(Error::InputConflict);
        }

        let send_permit = StartedScopeDispatchPermit::after_committed_start(
            &started,
            &authorization,
            &prepared_attempt,
        )?;

        let continuation = crate::AdvisoryDispatchContinuation::after_committed_start(
            identity.tenant_id,
            workspace.id,
            &opportunity,
            &started,
            &authorization,
        )?;
        // Clone immutable DATA only; the one-use permit moves into transport.
        let sealed_prepared = prepared_attempt.clone();
        let monotonic_start = std::time::Instant::now();
        let raw = self
            .scope_advice_provider
            .observe_prepared(
                &ScopeAdviceProviderRequest {
                    dispatch_id,
                    request: typed_request,
                    budget_policy: policy,
                },
                prepared_attempt,
                send_permit,
            )
            .await
            .unwrap_or_else(|error| {
                crate::ScopeAdviceRawObservation::from_legacy(provider_error_observation(error))
            });
        let elapsed = i64::try_from(monotonic_start.elapsed().as_millis()).unwrap_or(i64::MAX);
        // No caller/session reauthorization may precede durable retention/accounting.
        let saved = self
            .seal_committed_advisory_observation(&continuation, &raw.receipt, elapsed)
            .await?;
        let usage = self
            .scope_advice_provider
            .usage_from_sealed_response(&saved)
            .unwrap_or_default();
        let (saved, consumption) = self
            .consume_committed_advisory_observation(&continuation, usage)
            .await?;
        self.finish_scope_receipt(
            context,
            request,
            &saved,
            consumption,
            sealed_prepared,
            &manifest,
            raw.legacy_answers,
        )
        .await
    }
}
