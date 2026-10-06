use super::*;

pub(super) struct RunScopeContext {
    pub(super) identity: tect_domain::HostIdentity,
    pub(super) workspace: tect_domain::Workspace,
    pub(super) session: tect_domain::Session,
    pub(super) config: tect_domain::WorkspaceAdvisoryConfig,
    pub(super) authored_request_digest: Option<String>,
    pub(super) authority_request: ScopeAuthorityRequest,
    pub(super) observation: crate::ScopeAuthorityObservation,
    pub(super) manifest: tect_domain::ScopeConstructorManifest,
}

pub(super) struct PreparedScopeContext {
    pub(super) run: RunScopeContext,
    pub(super) opportunity: AdvisoryOpportunity,
    pub(super) prepared_attempt: PreparedScopeAdviceAttempt,
    pub(super) typed_request: ScopeAdviceRequest,
    pub(super) policy: crate::ScopeBudgetPolicyEvaluation,
    pub(super) verified_policy: tect_domain::AdvisoryBudgetPolicy,
}

impl WorkspaceService {
    pub(super) async fn prepare_scope_advisory(
        &self,
        context: &RequestContext,
        request: &RunScopeAdvisory,
        run: RunScopeContext,
    ) -> Result<ScopeAdvisoryOutcome> {
        let RunScopeContext {
            identity,
            workspace,
            session,
            config,
            authored_request_digest,
            authority_request,
            observation,
            manifest,
        } = run;
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
        let existing_receipt_opportunity = prepare
            .advisory_opportunity_by_request(workspace.id, &request.request_id.to_string())
            .await?;
        if let Some((saved, stored)) = self
            .scope_receipt_for_replay(
                &mut *prepare,
                workspace.id,
                fresh_identity.principal_id,
                fresh_session.id,
                request,
                existing_receipt_opportunity.as_ref(),
            )
            .await?
        {
            prepare.commit().await?;
            return self
                .recover_scope_receipt(context, request, fresh_identity.tenant_id, saved, stored)
                .await;
        }
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
                            ScopeCaptureIdentity {
                                actor: identity.principal_id,
                                session: session.id,
                                material_digest,
                            },
                            ScopeCaptureStatus {
                                state: AdvisoryOpportunityState::NoCall,
                                reason,
                            },
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

        let verified_policy = crate::matrix_advisory_capture::lookup_verified_matrix_budget(
            prepare.advisory_budget_policy_store(),
            workspace.id,
        )
        .await?;
        let policy = match verified_policy.as_ref() {
            Some(verified) => self
                .scope_budget
                .evaluate(
                    &ScopeBudgetRequest {
                        workspace_id: workspace.id,
                        actor_id: identity.principal_id,
                        candidate_set_id: request.candidate_set_id,
                        config_revision: config.revision,
                        manifest_digest: manifest.whole_set_digest.clone(),
                        body_length: prepared_attempt.body_length(),
                        body_sha256: prepared_attempt.body_sha256().to_owned(),
                    },
                    verified,
                )
                .await?
                .filter(|policy| policy.policy_id == verified.id().to_string()),
            None => None,
        };
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
            let opportunity = prepare
                .capture_advisory_opportunity(
                    workspace.id,
                    &scope_opportunity_input(
                        request,
                        &config,
                        ScopeCaptureIdentity {
                            actor: identity.principal_id,
                            session: session.id,
                            material_digest,
                        },
                        ScopeCaptureStatus {
                            state: AdvisoryOpportunityState::NoCall,
                            reason: AdvisoryReason::BudgetPolicyInvalid,
                        },
                        Some(manifest.source.candidate_set_revision),
                    ),
                )
                .await?;
            prepare.commit().await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity,
                advice: None,
            });
        };
        let verified_policy = verified_policy.ok_or(Error::InternalInvariant)?;

        let input = scope_opportunity_input(
            request,
            &config,
            ScopeCaptureIdentity {
                actor: identity.principal_id,
                session: session.id,
                material_digest: manifest.whole_set_digest.clone(),
            },
            ScopeCaptureStatus {
                state: AdvisoryOpportunityState::Prepared,
                reason: AdvisoryReason::DispatchAuthorized,
            },
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

        if authored_request_digest.is_some() {
            let current_source = match self.scope_authority.observe(&authority_request).await {
                Ok(crate::ScopeAuthorityOutcome::Authorized(value))
                    if validate_observation(&authority_request, &value).is_ok()
                        && *value == observation =>
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

        self.dispatch_scope_advisory(
            context,
            request,
            PreparedScopeContext {
                run: RunScopeContext {
                    identity,
                    workspace,
                    session,
                    config,
                    authored_request_digest,
                    authority_request,
                    observation,
                    manifest,
                },
                opportunity,
                prepared_attempt,
                typed_request,
                policy,
                verified_policy,
            },
        )
        .await
    }
}
