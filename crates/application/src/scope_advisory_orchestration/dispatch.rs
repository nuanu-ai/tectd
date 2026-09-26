use super::*;

pub(super) struct PreparedScopeDispatch<'a> {
    pub(super) context: &'a RequestContext,
    pub(super) request: &'a RunScopeAdvisory,
    pub(super) config: &'a tect_domain::WorkspaceAdvisoryConfig,
    pub(super) workspace_id: Uuid,
    pub(super) tenant_id: Uuid,
    pub(super) authority_request: &'a ScopeAuthorityRequest,
    pub(super) observation: &'a crate::ScopeAuthorityObservation,
    pub(super) manifest: &'a tect_domain::ScopeConstructorManifest,
    pub(super) opportunity: AdvisoryOpportunity,
    pub(super) prepared_attempt: PreparedScopeAdviceAttempt,
    pub(super) typed_request: ScopeAdviceRequest,
    pub(super) policy: crate::ScopeBudgetPolicyEvaluation,
    pub(super) authored: bool,
}

impl WorkspaceService {
    pub(super) async fn dispatch_prepared_scope_advisory(
        &self,
        prepared: PreparedScopeDispatch<'_>,
    ) -> Result<ScopeAdvisoryOutcome> {
        let PreparedScopeDispatch {
            context,
            request,
            config,
            workspace_id,
            tenant_id,
            authority_request,
            observation,
            manifest,
            opportunity,
            prepared_attempt,
            typed_request,
            policy,
            authored,
        } = prepared;
        if authored {
            let current_source = match self.scope_authority.observe(authority_request).await {
                Ok(crate::ScopeAuthorityOutcome::Authorized(value))
                    if validate_observation(authority_request, &value).is_ok()
                        && value.as_ref() == observation =>
                {
                    supply_scope_manifest(
                        self.scope_manifest_supplier.as_ref(),
                        tenant_id,
                        &value,
                        request,
                    )
                    .await
                    .is_ok_and(|fresh| &fresh == manifest)
                }
                _ => false,
            };
            let (mut recheck, _, _) = self
                .scope_transaction(context, TransactionMode::ReadWrite)
                .await?;
            let current_config = recheck.advisory_config(workspace_id).await?;
            let current_manifest = recheck
                .scope_advisory_manifest(workspace_id, opportunity.id)
                .await?;
            let reason = if current_config != *config {
                Some(AdvisoryReason::ConfigurationChanged)
            } else if !current_source || current_manifest.as_ref() != Some(manifest) {
                Some(AdvisoryReason::DeterministicInputInvalid)
            } else {
                None
            };
            if let Some(reason) = reason {
                let disposition = crate::ScopePreparedAdvisoryDisposition {
                    opportunity_id: opportunity.id,
                    candidate_set_id: request.candidate_set_id,
                    expected_source_digest: manifest.source.digest.clone(),
                    reason,
                };
                recheck
                    .finalize_prepared_scope_advisory_without_dispatch(workspace_id, &disposition)
                    .await?;
                recheck.commit().await?;
                return Ok(ScopeAdvisoryOutcome {
                    opportunity: AdvisoryOpportunity {
                        state: if reason == AdvisoryReason::ConfigurationChanged {
                            AdvisoryOpportunityState::Invalidated
                        } else {
                            AdvisoryOpportunityState::NoCall
                        },
                        primary_reason: reason,
                        ..opportunity
                    },
                    advice: None,
                });
            }
            recheck.commit().await?;
        }

        let dispatch_id = Uuid::new_v4();
        let (provider_name, adapter_version) = self
            .scope_advice_provider
            .identity()
            .ok_or(Error::TransportUnavailable)?;
        let authorization = scope_dispatch_authorization(
            &prepared_attempt,
            dispatch_id,
            opportunity.id,
            ScopeDispatchProvider {
                name: provider_name,
                adapter_version,
            },
            config,
            opportunity.material_digest.clone(),
            &policy,
        )?;
        let lifecycle = AdvisoryLifecycleCapability::internal();
        let (mut authorize, _, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let authorized = match authorize
            .authorize_advisory_dispatch(&lifecycle, workspace_id, config.revision, &authorization)
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
                            workspace_id,
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
            .start_advisory_dispatch(&lifecycle, workspace_id, authorized.id)
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
                if terminal_workspace.id != workspace_id {
                    return Err(Error::InputConflict);
                }
                let terminal = terminal_tx
                    .advisory_opportunity_for_dispatch(workspace_id, opportunity.id)
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
            tenant_id,
            workspace_id,
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
            manifest,
            raw.legacy_answers,
        )
        .await
    }
}
