use crate::{
    AdvisoryLifecycleCapability, AuthoredScopeSet, GuardedScopeAdviceRecord,
    PreparedScopeAdviceAttempt, ScopeAdviceProviderRequest, ScopeAuthorityRequest,
    ScopeBudgetRequest, ScopeManifestRecord, Sha256ScopeDigest, TransactionMode, WorkspaceService,
};
mod arguments;
mod capture;
mod decisions;
mod dispatch;
mod helpers;
mod prepare;
mod receipts;
mod recovery;
use arguments::*;
use helpers::*;
use prepare::RunScopeContext;
use serde::{Deserialize, Serialize};
use tect_domain::{
    AdvisoryDispatchAuthorization, AdvisoryDispatchOutcome, AdvisoryDispatchStart,
    AdvisoryDispatchState, AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryPolicyInput,
    AdvisoryReason, AdvisoryRequestPreference, AdvisorySendCertainty, Error, GuardedScopeAdvice,
    RequestContext, Result, ScopeAdviceRequest, assess_advisory_policy, guard_scope_advice,
};
use uuid::Uuid;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunScopeAdvisory {
    pub request_id: Uuid,
    pub candidate_set_id: Uuid,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authored_scope_set: Option<AuthoredScopeSet>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAdvisoryOutcome {
    pub opportunity: AdvisoryOpportunity,
    pub advice: Option<GuardedScopeAdvice>,
}

/// One-use transport capability minted only after the audited dispatch start
/// transaction has committed. Its private constructor is in this module.
pub struct StartedScopeDispatchPermit {
    dispatch_id: Uuid,
    body_sha256: String,
    body_length: usize,
    profile: String,
    model: String,
    destination: String,
    wire_version: String,
    configuration_digest: String,
}

impl StartedScopeDispatchPermit {
    fn after_committed_start(
        started: &AdvisoryDispatchStart,
        authorization: &AdvisoryDispatchAuthorization,
        prepared: &PreparedScopeAdviceAttempt,
    ) -> Result<Self> {
        let dispatch = &started.dispatch;
        let reservation = started
            .budget_reservation
            .as_ref()
            .ok_or(Error::BudgetPolicyInvalid)?;
        let policy_id = reservation.policy_id.to_string();
        let expected_policy = serde_json::json!({
            "policy_id": policy_id,
            "policy_version": reservation.policy_version,
            "policy_digest": reservation.policy_digest,
        });
        if authorization
            .configuration_snapshot
            .get("budget_policy_id")
            .and_then(serde_json::Value::as_str)
            != Some(policy_id.as_str())
            || authorization.configuration_snapshot.get("budget_policy") != Some(&expected_policy)
        {
            return Err(Error::BudgetPolicyInvalid);
        }
        if !started.should_send
            || reservation.dispatch_id != dispatch.id
            || reservation.request_sha256 != authorization.payload_digest
            || reservation.request_utf8_bytes != i64::try_from(prepared.body_length()).unwrap_or(-1)
            || reservation.reserved_calls != 1
            || dispatch.state != AdvisoryDispatchState::Sending
            || dispatch.send_certainty != AdvisorySendCertainty::SentUnknown
            || dispatch.id != authorization.dispatch_id
            || dispatch.opportunity_id != authorization.opportunity_id
            || dispatch.configuration_digest != authorization.configuration_digest
            || dispatch.payload_digest != authorization.payload_digest
            || dispatch.model != prepared.model()
            || authorization.request_payload != prepared.body()
            || authorization.payload_digest != prepared.body_sha256()
            || prepared.body_length() != prepared.body().len()
        {
            return Err(Error::InputConflict);
        }
        Ok(Self {
            dispatch_id: dispatch.id,
            body_sha256: prepared.body_sha256().to_owned(),
            body_length: prepared.body_length(),
            profile: prepared.profile().to_owned(),
            model: prepared.model().to_owned(),
            destination: prepared.destination().to_owned(),
            wire_version: prepared.wire_version().to_owned(),
            configuration_digest: dispatch.configuration_digest.clone(),
        })
    }

    pub fn permits(&self, dispatch_id: Uuid, prepared: &PreparedScopeAdviceAttempt) -> bool {
        self.dispatch_id == dispatch_id
            && self.body_sha256 == prepared.body_sha256()
            && self.body_length == prepared.body_length()
            && self.profile == prepared.profile()
            && self.model == prepared.model()
            && self.destination == prepared.destination()
            && self.wire_version == prepared.wire_version()
            && !self.configuration_digest.is_empty()
    }
}

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
        let existing = read
            .advisory_opportunity_by_request(workspace.id, &request.request_id.to_string())
            .await?;
        let mut bound_request = request.clone();
        bound_request.session_preference = bound_session_preference(
            &mut *read,
            workspace.id,
            session.id,
            identity.principal_id,
            existing.as_ref(),
        )
        .await?;
        let request = &bound_request;
        if let Some((saved, stored)) = self
            .scope_receipt_for_replay(
                &mut *read,
                workspace.id,
                identity.principal_id,
                session.id,
                request,
                existing.as_ref(),
            )
            .await?
        {
            read.commit().await?;
            return self
                .recover_scope_receipt(context, request, identity.tenant_id, saved, stored)
                .await;
        }
        let config = read.advisory_config(workspace.id).await?;
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
                    ScopeCaptureIdentity {
                        actor: identity.principal_id,
                        session: session.id,
                        material_digest: digest,
                    },
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
            crate::ScopeAuthorityOutcome::Authorized(value) => *value,
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
                    ScopeCaptureIdentity {
                        actor: identity.principal_id,
                        session: session.id,
                        material_digest,
                    },
                    ScopeCaptureStatus {
                        state: preliminary.state,
                        reason: preliminary.reason,
                    },
                    Some(manifest.source.candidate_set_revision),
                )
                .await?;
            return Ok(ScopeAdvisoryOutcome {
                opportunity,
                advice: None,
            });
        }

        self.prepare_scope_advisory(
            context,
            request,
            RunScopeContext {
                identity,
                workspace,
                session,
                config,
                authored_request_digest,
                authority_request,
                observation,
                manifest,
            },
        )
        .await
    }
}
