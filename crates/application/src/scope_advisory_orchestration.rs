use crate::{
    AdvisoryLifecycleCapability, GuardedScopeAdviceRecord, ScopeAdviceProviderRequest,
    ScopeAuthorityRequest, ScopeBudgetRequest, ScopeManifestRecord, Sha256ScopeDigest,
    TransactionMode, WorkspaceService,
};
mod capture;
mod decisions;
mod helpers;
use helpers::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tect_domain::{
    AdvisoryDispatchAuthorization, AdvisoryDispatchOutcome, AdvisoryDispatchSeal,
    AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryPolicyInput, AdvisoryReason,
    AdvisoryRequestPreference, AdvisoryRetryBasis, AdvisorySendCertainty, Error,
    GuardedScopeAdvice, RequestContext, Result, ScopeAdviceRequest, ScopeDispositionRequest,
    ScopeDispositionRevision, assess_advisory_policy, guard_scope_advice,
};
use uuid::Uuid;

#[cfg(test)]
mod tests;

/// Caller-authored alternatives are request-local until the source resolver
/// freezes them against the authoritative candidate set and its fragments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthoredScopeAlternative {
    pub key: String,
    pub kind: tect_domain::ScopeDecompositionKind,
    pub draft: tect_domain::ScopeCandidateDraft,
    /// Explicit structural coverage claims; the resolver must verify them.
    pub covered_source_ref_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthoredScopeSet {
    pub expected_candidate_set_revision: i64,
    pub baseline_key: String,
    pub alternatives: Vec<AuthoredScopeAlternative>,
}

impl AuthoredScopeSet {
    pub fn validate(&self) -> Result<()> {
        if self.expected_candidate_set_revision < 1
            || self.alternatives.is_empty()
            || self.alternatives.len() > 100
        {
            return Err(Error::InvalidArguments);
        }
        let mut keys = BTreeSet::new();
        for alternative in &self.alternatives {
            if !valid_request_local_key(&alternative.key)
                || !keys.insert(&alternative.key)
                || alternative.covered_source_ref_ids.iter().any(Uuid::is_nil)
                || alternative
                    .covered_source_ref_ids
                    .windows(2)
                    .any(|pair| pair[0] >= pair[1])
            {
                return Err(Error::InvalidArguments);
            }
            alternative.draft.validate()?;
        }
        if !keys.contains(&self.baseline_key) {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

fn valid_request_local_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

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

        // Source-authored active material requires the resolver's authoritative
        // fragment and candidate-set binding before any provider dispatch.
        if request.authored_scope_set.is_some() {
            return Err(Error::InputPending);
        }

        let authority_request = ScopeAuthorityRequest {
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
        let manifest = match self.scope_manifest_supplier.supply(&observation).await {
            Ok(value)
                if value.validate(&Sha256ScopeDigest).is_ok()
                    && value.source == observation.source
                    && value.obligations == observation.obligations =>
            {
                value
            }
            _ => {
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
            let opportunity = self
                .capture_scope_opportunity(
                    context,
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                    manifest.whole_set_digest.clone(),
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
            let opportunity = self
                .capture_scope_opportunity(
                    context,
                    request,
                    &config,
                    identity.principal_id,
                    session.id,
                    manifest.whole_set_digest.clone(),
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
        let stored = prepare
            .prepare_scope_advisory_manifest(
                workspace.id,
                &ScopeManifestRecord {
                    opportunity_id: opportunity.id,
                    candidate_set_id: request.candidate_set_id,
                    config_revision: config.revision,
                    opportunity_material_digest: opportunity.material_digest.clone(),
                    manifest: manifest.clone(),
                },
            )
            .await?;
        if stored != manifest {
            return Err(Error::InputConflict);
        }
        prepare.commit().await?;

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
        authorize
            .authorize_advisory_dispatch(&lifecycle, workspace.id, config.revision, &authorization)
            .await?;
        authorize.commit().await?;
        let (mut start, _, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let started = start
            .start_advisory_dispatch(&lifecycle, workspace.id, dispatch_id)
            .await?;
        start.commit().await?;
        if !started.should_send {
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
                self.scope_manifest_supplier.supply(value).await.ok()
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
