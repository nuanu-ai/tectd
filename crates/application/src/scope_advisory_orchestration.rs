use crate::{
    AdvisoryLifecycleCapability, AuthoredScopeSet, GuardedScopeAdviceRecord,
    ScopeAdviceProviderRequest, ScopeAuthorityRequest, ScopeBudgetRequest, ScopeManifestRecord,
    Sha256ScopeDigest, TransactionMode, WorkspaceService,
};
mod capture;
mod decisions;
mod helpers;
use helpers::*;
use serde::{Deserialize, Serialize};
use tect_domain::{
    AdvisoryDispatchAuthorization, AdvisoryDispatchOutcome, AdvisoryDispatchSeal,
    AdvisoryDispatchState, AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryPolicyInput,
    AdvisoryReason, AdvisoryRequestPreference, AdvisoryRetryBasis, AdvisorySendCertainty, Error,
    GuardedScopeAdvice, RequestContext, Result, ScopeAdviceRequest, ScopeDispositionRequest,
    ScopeDispositionRevision, assess_advisory_policy, guard_scope_advice,
};
use uuid::Uuid;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunScopeAdvisory {
    pub request_id: Uuid,
    pub candidate_set_id: Uuid,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_scope_set: Option<AuthoredScopeSet>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScopeAdvisoryOutcome {
    pub opportunity: AdvisoryOpportunity,
    pub advice: Option<GuardedScopeAdvice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecideScopeAdvisory {
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub request: ScopeDispositionRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreserveScopeAdvisory {
    pub receipt_id: Uuid,
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub manifest: tect_domain::ScopeConstructorManifest,
    pub advice: GuardedScopeAdvice,
    pub disposition: ScopeDispositionRevision,
}

impl WorkspaceService {
    pub(crate) async fn run_scope_advisory(
        &self,
        context: &RequestContext,
        request: &RunScopeAdvisory,
    ) -> Result<ScopeAdvisoryOutcome> {
        if request.request_id.is_nil() || request.candidate_set_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        if let Some(authored) = &request.authored_scope_set {
            authored.validate()?;
        }
        let (mut read, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, session) = Self::bound_session(&mut *read, context, &identity).await?;
        let config = read.advisory_config(workspace.id).await?;
        let existing = read
            .advisory_opportunity_by_request(workspace.id, &request.request_id.to_string())
            .await?;
        // Disabled and explicitly skipped requests do not need source material.
        // The workspace-scoped candidate lookup still proves target access.
        let early_no_call =
            early_no_call_target(&mut *read, workspace.id, &config, request).await?;
        let authored_request_digest = if early_no_call.is_none() {
            request
                .authored_scope_set
                .as_ref()
                .map(authored_request_digest)
                .transpose()?
        } else {
            None
        };
        let stored_authored_manifest = if authored_request_digest.is_some() {
            read.scope_advisory_manifest_by_request_key(
                workspace.id,
                &request.request_id.to_string(),
            )
            .await?
        } else {
            None
        };
        let existing_authored_opportunity =
            if stored_authored_manifest.is_some() && existing.is_none() {
                read.advisory_opportunity_by_request(workspace.id, &request.request_id.to_string())
                    .await?
            } else {
                existing.clone()
            };
        read.commit().await?;

        if let Some((reason, revision)) = early_no_call {
            let digest = no_call_digest(request, config.revision, reason)?;
            if let Some(existing) = existing {
                if existing.target_kind != "scope_candidate_set"
                    || existing.target_id != Some(request.candidate_set_id)
                    || existing.work_revision != Some(revision)
                    || existing.config_revision != config.revision
                    || existing.material_digest != digest
                {
                    return Err(Error::InputConflict);
                }
                return Ok(ScopeAdvisoryOutcome {
                    opportunity: existing,
                    advice: None,
                });
            }
            let opportunity = self
                .capture_early_scope_no_call(
                    context,
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                    digest,
                    reason,
                    revision,
                )
                .await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity,
                advice: None,
            });
        }

        if let Some(authored_request_digest) = authored_request_digest.as_deref() {
            if let Some(stored) = stored_authored_manifest.as_ref() {
                let opportunity = validate_authored_replay_binding(
                    stored,
                    existing_authored_opportunity.as_ref(),
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                    authored_request_digest,
                )?
                .clone();
                return self
                    .replay_authored_scope_advisory(
                        context,
                        workspace.id,
                        opportunity,
                        &stored.record,
                    )
                    .await;
            }
            // Without a persisted manifest, replay only a no-call whose
            // material digest proves this exact authored request; legacy or
            // changed requests under the same key conflict.
            if let Some(existing) = existing.as_ref() {
                return Ok(ScopeAdvisoryOutcome {
                    opportunity: validate_authored_no_call_replay(
                        Some(existing),
                        request,
                        &config,
                        identity.principal_id,
                        session.id,
                    )?
                    .clone(),
                    advice: None,
                });
            }
        }

        let authority_request = ScopeAuthorityRequest {
            tenant_id: identity.tenant_id,
            workspace_id: workspace.id,
            actor_id: identity.principal_id,
            session_id: session.id,
            candidate_set_id: request.candidate_set_id,
        };
        let observation = match self.scope_authority.observe(&authority_request).await? {
            crate::ScopeAuthorityOutcome::Authorized(value) => value,
            crate::ScopeAuthorityOutcome::AuthorizedInvalid(value) => {
                validate_invalid_observation(&authority_request, &value)?;
                return self
                    .capture_invalid_scope_input(
                        context,
                        request,
                        &config,
                        identity.principal_id,
                        session.id,
                    )
                    .await;
            }
        };
        validate_observation(&authority_request, &observation)?;
        if observation.source.validate(&Sha256ScopeDigest).is_err() {
            return self
                .capture_invalid_scope_input(
                    context,
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                )
                .await;
        }
        if self.scope_manifest_supplier.identity().is_none() {
            return self
                .capture_invalid_scope_input(
                    context,
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                )
                .await;
        }
        let manifest = match supply_scope_manifest(
            self.scope_manifest_supplier.as_ref(),
            identity.tenant_id,
            &observation,
            request,
        )
        .await
        {
            Ok(value) => value,
            Err(_) => {
                return self
                    .capture_invalid_scope_input(
                        context,
                        request,
                        &config,
                        identity.principal_id,
                        session.id,
                    )
                    .await;
            }
        };
        if let Some(existing) = existing {
            if existing.target_kind != "scope_candidate_set"
                || existing.target_id != Some(request.candidate_set_id)
                || existing.work_revision != Some(manifest.source.candidate_set_revision)
                || existing.config_revision != config.revision
                || existing.material_digest != manifest.whole_set_digest
            {
                return Err(Error::InputConflict);
            }
            let advice = if existing.state == AdvisoryOpportunityState::Advised {
                let (mut replay, _, _) = self
                    .scope_transaction(context, TransactionMode::ReadOnly)
                    .await?;
                let advice = replay
                    .guarded_scope_advice(workspace.id, existing.id)
                    .await?
                    .ok_or(Error::StorageUnavailable)?;
                tect_domain::validate_guarded_advice_binding(
                    &Sha256ScopeDigest,
                    &manifest,
                    &advice,
                )?;
                replay.commit().await?;
                Some(advice)
            } else {
                None
            };
            return Ok(ScopeAdvisoryOutcome {
                opportunity: existing,
                advice,
            });
        }
        let preliminary = assess_advisory_policy(AdvisoryPolicyInput {
            workspace_mode: config.mode,
            session_preference: request.session_preference,
            request_preference: request.request_preference,
            deterministic_input_valid: true,
            capability_available: self.scope_advice_provider.identity().is_some(),
            provider_configured: config.provider_configured(),
        });
        if preliminary.state == AdvisoryOpportunityState::NoCall {
            let material_digest = if authored_request_digest.is_some() {
                no_call_digest(request, config.revision, preliminary.reason)?
            } else {
                manifest.whole_set_digest.clone()
            };
            let opportunity = self
                .capture_scope_opportunity(
                    context,
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                    material_digest,
                    preliminary.state,
                    preliminary.reason,
                    Some(manifest.source.candidate_set_revision),
                )
                .await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity,
                advice: None,
            });
        }
        let policy = self
            .scope_budget
            .evaluate(&ScopeBudgetRequest {
                workspace_id: workspace.id,
                actor_id: identity.principal_id,
                candidate_set_id: request.candidate_set_id,
                config_revision: config.revision,
                manifest_digest: manifest.whole_set_digest.clone(),
            })
            .await?;
        let Some(policy) = policy else {
            let material_digest = if authored_request_digest.is_some() {
                no_call_digest(
                    request,
                    config.revision,
                    AdvisoryReason::CapabilityUnavailable,
                )?
            } else {
                manifest.whole_set_digest.clone()
            };
            let opportunity = self
                .capture_scope_opportunity(
                    context,
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                    material_digest,
                    AdvisoryOpportunityState::NoCall,
                    AdvisoryReason::CapabilityUnavailable,
                    Some(manifest.source.candidate_set_revision),
                )
                .await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity,
                advice: None,
            });
        };

        let (mut prepare, fresh_identity) =
            self.authorized(context, TransactionMode::ReadWrite).await?;
        prepare
            .lock_native_session(fresh_identity.host_id, &context.native_session_id)
            .await?;
        let (fresh_workspace, fresh_session) =
            Self::bound_session(&mut *prepare, context, &fresh_identity).await?;
        if fresh_workspace.id != workspace.id
            || fresh_session.id != session.id
            || prepare.advisory_config(workspace.id).await? != config
            || prepare
                .candidate_context(workspace.id, request.candidate_set_id)
                .await?
                .is_none()
        {
            return Err(Error::StaleRevision);
        }
        let input = scope_opportunity_input(
            request,
            &config,
            identity.principal_id,
            session.id,
            manifest.whole_set_digest.clone(),
            AdvisoryOpportunityState::Prepared,
            AdvisoryReason::DispatchAuthorized,
            Some(manifest.source.candidate_set_revision),
        );
        let opportunity = prepare
            .capture_advisory_opportunity(workspace.id, &input)
            .await?;
        if let Some(authored_request_digest) = authored_request_digest.as_deref() {
            // Close the read/resolve/write race: a concurrent identical call
            // may have committed the same request key while this request was
            // resolving. Replay it without authorizing a second provider send.
            if let Some(stored) = prepare
                .scope_advisory_manifest_by_request_key(
                    workspace.id,
                    &request.request_id.to_string(),
                )
                .await?
            {
                let opportunity = validate_authored_replay_binding(
                    &stored,
                    Some(&opportunity),
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                    authored_request_digest,
                )?
                .clone();
                prepare.commit().await?;
                return self
                    .replay_authored_scope_advisory(
                        context,
                        workspace.id,
                        opportunity,
                        &stored.record,
                    )
                    .await;
            }
        }
        let manifest_record = ScopeManifestRecord {
            opportunity_id: opportunity.id,
            candidate_set_id: request.candidate_set_id,
            config_revision: config.revision,
            opportunity_material_digest: opportunity.material_digest.clone(),
            manifest: manifest.clone(),
        };
        let stored = if let Some(authored_request_digest) = authored_request_digest.as_deref() {
            prepare
                .prepare_authored_scope_advisory_manifest(
                    workspace.id,
                    &manifest_record,
                    authored_request_digest,
                )
                .await?
        } else {
            prepare
                .prepare_scope_advisory_manifest(workspace.id, &manifest_record)
                .await?
        };
        if stored != manifest {
            return Err(Error::InputConflict);
        }
        prepare.commit().await?;

        if authored_request_digest.is_some() {
            let current_source = match self.scope_authority.observe(&authority_request).await {
                Ok(crate::ScopeAuthorityOutcome::Authorized(value))
                    if validate_observation(&authority_request, &value).is_ok()
                        && value == observation =>
                {
                    supply_scope_manifest(
                        self.scope_manifest_supplier.as_ref(),
                        identity.tenant_id,
                        &value,
                        request,
                    )
                    .await
                    .is_ok_and(|fresh| fresh == manifest)
                }
                _ => false,
            };
            let (mut recheck, _, _) = self
                .scope_transaction(context, TransactionMode::ReadWrite)
                .await?;
            let current_config = recheck.advisory_config(workspace.id).await?;
            let current_manifest = recheck
                .scope_advisory_manifest(workspace.id, opportunity.id)
                .await?;
            let reason = if current_config != config {
                Some(AdvisoryReason::ConfigurationChanged)
            } else if !current_source || current_manifest.as_ref() != Some(&manifest) {
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
                    .finalize_prepared_scope_advisory_without_dispatch(workspace.id, &disposition)
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

        let typed_request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest)?;
        let payload = serde_json::to_vec(&typed_request).map_err(Error::invalid_arguments_from)?;
        let dispatch_id = Uuid::new_v4();
        let (provider_name, adapter_version) = self
            .scope_advice_provider
            .identity()
            .ok_or(Error::TransportUnavailable)?;
        let profile = config
            .provider_profile_ref
            .clone()
            .ok_or(Error::InvalidConfiguration)?;
        let model = config
            .model_configuration
            .clone()
            .ok_or(Error::InvalidConfiguration)?;
        let configuration_snapshot = serde_json::json!({
            "provider_profile_ref": profile,
            "model_configuration": model,
            "adapter_version": adapter_version,
            "budget_policy_id": policy.policy_id,
        });
        let configuration_bytes =
            serde_json::to_vec(&configuration_snapshot).map_err(Error::invalid_arguments_from)?;
        let authorization = AdvisoryDispatchAuthorization {
            dispatch_id,
            opportunity_id: opportunity.id,
            predecessor_dispatch_id: None,
            attempt_number: 1,
            retry_basis: AdvisoryRetryBasis::Initial,
            provider: provider_name.into(),
            model: model.model,
            configuration_snapshot,
            configuration_digest: sha256(&configuration_bytes),
            material_digest: opportunity.material_digest.clone(),
            payload_digest: sha256(&payload),
            request_payload: payload,
        };
        authorization.validate()?;
        let lifecycle = AdvisoryLifecycleCapability::internal();
        let (mut authorize, _, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
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
            .start_advisory_dispatch(&lifecycle, workspace.id, authorized.id)
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

        let mut provider_observation = self
            .scope_advice_provider
            .attempt(&ScopeAdviceProviderRequest {
                dispatch_id,
                request: typed_request.clone(),
                budget_policy: policy,
            })
            .await
            .map(normalize_provider_success)
            .unwrap_or_else(provider_error_observation);
        let guarded = if provider_observation.outcome == AdvisoryDispatchOutcome::ProviderResponse
            && provider_observation.send_certainty == AdvisorySendCertainty::Sent
            && provider_observation.response_payload.is_some()
        {
            provider_observation.answers.as_ref().and_then(|answers| {
                guard_scope_advice(&Sha256ScopeDigest, &manifest, &typed_request, answers).ok()
            })
        } else {
            None
        };
        if provider_observation.outcome == AdvisoryDispatchOutcome::ProviderResponse
            && guarded.is_none()
        {
            provider_observation.outcome = AdvisoryDispatchOutcome::ProviderFailure;
            provider_observation.answers = None;
        }
        let seal = AdvisoryDispatchSeal {
            dispatch_id,
            send_certainty: provider_observation.send_certainty,
            outcome: provider_observation.outcome,
            response_payload: provider_observation.response_payload.clone(),
            input_tokens: provider_observation.input_tokens,
            output_tokens: provider_observation.output_tokens,
            latency_ms: provider_observation.latency_ms,
            raw_response_ref: provider_observation.raw_response_ref.clone(),
        };
        seal.validate()?;
        let (mut seal_tx, _, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let dispatch = seal_tx
            .seal_advisory_dispatch(&lifecycle, workspace.id, &seal)
            .await?;
        seal_tx.commit().await?;
        let Some(advice) = guarded else {
            let (mut finalize, _, _) = self
                .scope_transaction(context, TransactionMode::ReadWrite)
                .await?;
            let finalized = finalize
                .finalize_advisory_opportunity(
                    &lifecycle,
                    workspace.id,
                    opportunity.id,
                    config.revision,
                    &dispatch,
                )
                .await?;
            finalize.commit().await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity: finalized,
                advice: None,
            });
        };

        let fresh = self.scope_authority.observe(&authority_request).await;
        let fresh_manifest = match &fresh {
            Ok(crate::ScopeAuthorityOutcome::Authorized(value))
                if validate_observation(&authority_request, value).is_ok() =>
            {
                supply_scope_manifest(
                    self.scope_manifest_supplier.as_ref(),
                    identity.tenant_id,
                    value,
                    request,
                )
                .await
                .ok()
            }
            _ => None,
        };
        let (mut persist, _, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let current_manifest = persist
            .scope_advisory_manifest(workspace.id, opportunity.id)
            .await?;
        let fresh_valid = fresh.as_ref().is_ok_and(|value| {
            value == &crate::ScopeAuthorityOutcome::Authorized(observation.clone())
        }) && fresh_manifest
            .as_ref()
            .is_some_and(|value| value.validate(&Sha256ScopeDigest).is_ok() && value == &manifest);
        if !fresh_valid
            || persist.advisory_config(workspace.id).await? != config
            || current_manifest != Some(manifest.clone())
        {
            persist
                .invalidate_scope_advisory(workspace.id, opportunity.id)
                .await?;
            persist.commit().await?;
            return Err(Error::StaleRevision);
        }
        let advice = match persist
            .finalize_guarded_scope_advice(
                workspace.id,
                &GuardedScopeAdviceRecord {
                    opportunity_id: opportunity.id,
                    candidate_set_id: request.candidate_set_id,
                    dispatch_id,
                    dispatch_material_digest: dispatch.material_digest,
                    config_revision: config.revision,
                    advice,
                },
            )
            .await
        {
            Ok(advice) => advice,
            Err(_) => {
                drop(persist);
                let (mut invalidate, _, _) = self
                    .scope_transaction(context, TransactionMode::ReadWrite)
                    .await?;
                invalidate
                    .invalidate_scope_advisory(workspace.id, opportunity.id)
                    .await?;
                invalidate.commit().await?;
                return Err(Error::StaleRevision);
            }
        };
        persist.commit().await?;
        Ok(ScopeAdvisoryOutcome {
            opportunity: AdvisoryOpportunity {
                state: AdvisoryOpportunityState::Advised,
                primary_reason: AdvisoryReason::ProviderResponse,
                provider_called: true,
                ..opportunity
            },
            advice: Some(advice),
        })
    }

    async fn finalize_prepared_scope_stale(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        candidate_set_id: Uuid,
        expected_source_digest: &str,
        reason: AdvisoryReason,
    ) -> Result<ScopeAdvisoryOutcome> {
        let disposition = crate::ScopePreparedAdvisoryDisposition {
            opportunity_id,
            candidate_set_id,
            expected_source_digest: expected_source_digest.to_owned(),
            reason,
        };
        let (mut tx, workspace, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        if workspace.id != workspace_id {
            return Err(Error::InputConflict);
        }
        tx.finalize_prepared_scope_advisory_without_dispatch(workspace_id, &disposition)
            .await?;
        let opportunity = tx
            .advisory_opportunity_for_dispatch(workspace_id, opportunity_id)
            .await?;
        let opportunity = validate_terminalized_pre_dispatch_opportunity(opportunity)?;
        tx.commit().await?;
        Ok(ScopeAdvisoryOutcome {
            opportunity,
            advice: None,
        })
    }

    async fn replay_authored_scope_advisory(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        opportunity: AdvisoryOpportunity,
        record: &ScopeManifestRecord,
    ) -> Result<ScopeAdvisoryOutcome> {
        let advice = if opportunity.state == AdvisoryOpportunityState::Advised {
            let (mut replay, workspace, _) = self
                .scope_transaction(context, TransactionMode::ReadOnly)
                .await?;
            if workspace.id != workspace_id {
                return Err(Error::InputConflict);
            }
            let advice = replay
                .guarded_scope_advice(workspace_id, opportunity.id)
                .await?
                .ok_or(Error::StorageUnavailable)?;
            tect_domain::validate_guarded_advice_binding(
                &Sha256ScopeDigest,
                &record.manifest,
                &advice,
            )?;
            replay.commit().await?;
            Some(advice)
        } else {
            None
        };
        Ok(ScopeAdvisoryOutcome {
            opportunity,
            advice,
        })
    }

    async fn scope_transaction(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<(
        Box<dyn crate::UnitOfWork>,
        tect_domain::Workspace,
        tect_domain::Session,
    )> {
        let (mut tx, identity) = self.authorized(context, mode).await?;
        if mode == TransactionMode::ReadWrite {
            tx.lock_native_session(identity.host_id, &context.native_session_id)
                .await?;
        }
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        Ok((tx, workspace, session))
    }
}
