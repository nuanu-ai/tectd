use super::*;
use crate::StoredAdvisoryProviderReceipt;

impl WorkspaceService {
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn finish_scope_receipt(
        &self,
        context: &RequestContext,
        request: &RunScopeAdvisory,
        saved: &StoredAdvisoryProviderReceipt,
        consumption: tect_domain::AdvisoryBudgetConsumption,
        prepared: PreparedScopeAdviceAttempt,
        manifest: &tect_domain::ScopeConstructorManifest,
        legacy_answers: Option<tect_domain::NormalizedScopeAdviceAnswers>,
    ) -> Result<ScopeAdvisoryOutcome> {
        let opportunity = &saved.opportunity;
        let dispatch = &saved.dispatch;
        // Accounting is already committed. This is the first post-send caller fence.
        let (mut persist, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        persist
            .lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *persist, context, &identity).await?;
        if workspace.id != opportunity.workspace_id
            || identity.principal_id != opportunity.authorized_actor_id
            || session.id != opportunity.session_id
        {
            return Err(Error::Forbidden);
        }
        let raw_healthy = saved.observation.as_ref().is_some_and(|raw| {
            raw.response_complete
                && raw.response_payload.is_some()
                && raw
                    .original_transport_context
                    .as_ref()
                    .is_some_and(|transport| {
                        transport.send_certainty == AdvisorySendCertainty::Sent
                            && transport.outcome == AdvisoryDispatchOutcome::ProviderResponse
                    })
        });
        if consumption.unknown_usage || consumption.exhausted_after_response || !raw_healthy {
            let terminal = persist
                .finalize_scope_advisory_without_advice(
                    workspace.id,
                    opportunity.id,
                    opportunity.config_revision,
                    dispatch,
                )
                .await?;
            persist.commit().await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity: terminal,
                advice: None,
            });
        }
        let config = persist.advisory_config(workspace.id).await?;
        let config_current = config.revision == opportunity.config_revision
            && config.mode == tect_domain::WorkspaceAdvisoryMode::Optional
            && serde_json::to_value(&config.provider_profile_ref)
                .map_err(Error::invalid_arguments_from)?
                == saved.configuration_snapshot["provider_profile_ref"]
            && serde_json::to_value(&config.model_configuration)
                .map_err(Error::invalid_arguments_from)?
                == saved.configuration_snapshot["model_configuration"];
        let provider_current =
            self.scope_advice_provider
                .identity()
                .is_some_and(|(name, version)| {
                    name == dispatch.provider
                        && Some(version) == saved.configuration_snapshot["adapter_version"].as_str()
                });
        if !config_current || !provider_current {
            persist
                .invalidate_scope_advisory(workspace.id, opportunity.id)
                .await?;
            persist.commit().await?;
            return Err(Error::StaleRevision);
        }
        let authority_request = ScopeAuthorityRequest {
            tenant_id: identity.tenant_id,
            workspace_id: workspace.id,
            actor_id: identity.principal_id,
            session_id: session.id,
            candidate_set_id: request.candidate_set_id,
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
        if fresh_manifest.as_ref() != Some(manifest)
            || persist
                .scope_advisory_manifest(workspace.id, opportunity.id)
                .await?
                .as_ref()
                != Some(manifest)
        {
            persist
                .invalidate_scope_advisory(workspace.id, opportunity.id)
                .await?;
            persist.commit().await?;
            return Err(Error::StaleRevision);
        }
        let policy = crate::matrix_advisory_capture::lookup_verified_matrix_budget(
            persist.advisory_budget_policy_store(),
            workspace.id,
        )
        .await?;
        let budget_current = if let Some(verified) = policy.as_ref() {
            let evaluation = self
                .scope_budget
                .evaluate(
                    &ScopeBudgetRequest {
                        workspace_id: workspace.id,
                        actor_id: identity.principal_id,
                        candidate_set_id: request.candidate_set_id,
                        config_revision: config.revision,
                        manifest_digest: manifest.whole_set_digest.clone(),
                        body_length: prepared.body_length(),
                        body_sha256: prepared.body_sha256().to_owned(),
                    },
                    verified,
                )
                .await?;
            evaluation.is_some_and(|evaluation| {
                evaluation.policy_id == verified.id().to_string()
                    && saved.configuration_snapshot["budget_policy_id"] == verified.id().to_string()
                    && saved.configuration_snapshot["budget_policy"]
                        == serde_json::json!({
                            "policy_id": verified.id().to_string(),
                            "policy_version": verified.version(),
                            "policy_digest": verified.digest(),
                        })
            })
        } else {
            false
        };
        let provider_context =
            crate::ScopeAdviceProviderContext::from_manifest(prepared.request(), manifest)?;
        let prepared_current = self
            .scope_advice_provider
            .prepared_matches_context(&provider_context, &prepared);
        // All current effect/authority gates precede any normalized-answer interpretation.
        let answers = if budget_current && prepared_current {
            legacy_answers.or_else(|| {
                self.scope_advice_provider
                    .parse_sealed_response(&prepared, saved)
                    .ok()
            })
        } else {
            None
        };
        let guarded = answers.as_ref().and_then(|answers| {
            guard_scope_advice(
                &Sha256ScopeDigest,
                opportunity.id,
                manifest,
                prepared.request(),
                answers,
            )
            .ok()
        });
        let Some(advice) = guarded else {
            let terminal = persist
                .finalize_scope_advisory_without_advice(
                    workspace.id,
                    opportunity.id,
                    opportunity.config_revision,
                    dispatch,
                )
                .await?;
            persist.commit().await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity: terminal,
                advice: None,
            });
        };
        let advice = match persist
            .finalize_guarded_scope_advice(
                workspace.id,
                &GuardedScopeAdviceRecord {
                    opportunity_id: opportunity.id,
                    candidate_set_id: request.candidate_set_id,
                    dispatch_id: dispatch.id,
                    dispatch_material_digest: dispatch.material_digest.clone(),
                    config_revision: opportunity.config_revision,
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
                ..opportunity.clone()
            },
            advice: Some(advice),
        })
    }
}
