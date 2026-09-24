use super::*;

impl WorkspaceService {
    pub async fn run_scope_advisory(
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

        // An active invocation needs the caller's complete authored set.
        // Disabled and explicitly skipped invocations above remain auditable
        // without requiring source material or enabling the provider.
        if request.authored_scope_set.is_none() {
            return Err(Error::InputPending);
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
                    AdvisoryReason::BudgetPolicyInvalid,
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
                    AdvisoryReason::BudgetPolicyInvalid,
                    Some(manifest.source.candidate_set_revision),
                )
                .await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity,
                advice: None,
            });
        };

        let typed_request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest)?;
        let provider_context =
            crate::ScopeAdviceProviderContext::from_manifest(&typed_request, &manifest)?;
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

        // Recheck under the session lock before doing even pure wire
        // preparation. A completed replay must never serialize or send again.
        if let Some(authored_request_digest) = authored_request_digest.as_deref() {
            if let Some(stored) = prepare
                .scope_advisory_manifest_by_request_key(
                    workspace.id,
                    &request.request_id.to_string(),
                )
                .await?
            {
                let existing = prepare
                    .advisory_opportunity_by_request(workspace.id, &request.request_id.to_string())
                    .await?;
                let opportunity = validate_authored_replay_binding(
                    &stored,
                    existing.as_ref(),
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
            if let Some(existing) = prepare
                .advisory_opportunity_by_request(workspace.id, &request.request_id.to_string())
                .await?
            {
                let opportunity = validate_authored_no_call_replay(
                    Some(&existing),
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                )?
                .clone();
                prepare.commit().await?;
                return Ok(ScopeAdvisoryOutcome {
                    opportunity,
                    advice: None,
                });
            }
        }

        let prepared_attempt = match prepare_scope_advice_attempt(
            self.scope_advice_provider.as_ref(),
            &provider_context,
            &config,
        ) {
            Ok(value) => value,
            Err(reason) => {
                let material_digest = no_call_digest(request, config.revision, reason)?;
                let revision = (reason != AdvisoryReason::DeterministicInputInvalid)
                    .then_some(manifest.source.candidate_set_revision);
                let opportunity = prepare
                    .capture_advisory_opportunity(
                        workspace.id,
                        &scope_opportunity_input(
                            request,
                            &config,
                            identity.principal_id,
                            session.id,
                            material_digest,
                            AdvisoryOpportunityState::NoCall,
                            reason,
                            revision,
                        ),
                    )
                    .await?;
                prepare.commit().await?;
                return Ok(ScopeAdvisoryOutcome {
                    opportunity,
                    advice: None,
                });
            }
        };

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
        self.dispatch_prepared_scope_advisory(PreparedScopeDispatch {
            context,
            request,
            config: &config,
            workspace_id: workspace.id,
            tenant_id: identity.tenant_id,
            authority_request: &authority_request,
            observation: &observation,
            manifest: &manifest,
            opportunity,
            prepared_attempt,
            typed_request,
            policy,
            authored: authored_request_digest.is_some(),
        })
        .await
    }
}
