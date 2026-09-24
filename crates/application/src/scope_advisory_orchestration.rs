use crate::{
    AdvisoryLifecycleCapability, AuthoredScopeSet, GuardedScopeAdviceRecord,
    PreparedScopeAdviceAttempt, ScopeAdviceProviderRequest, ScopeAuthorityRequest,
    ScopeBudgetRequest, ScopeManifestRecord, Sha256ScopeDigest, TransactionMode, WorkspaceService,
};
mod capture;
mod decisions;
mod dispatch;
mod helpers;
mod run;
use dispatch::PreparedScopeDispatch;
use helpers::*;
use tect_domain::{
    AdvisoryDispatchAuthorization, AdvisoryDispatchOutcome, AdvisoryDispatchSeal,
    AdvisoryDispatchStart, AdvisoryDispatchState, AdvisoryOpportunity, AdvisoryOpportunityState,
    AdvisoryPolicyInput, AdvisoryReason, AdvisoryRequestPreference, AdvisorySendCertainty, Error,
    GuardedScopeAdvice, RequestContext, Result, ScopeAdviceRequest, ScopeDispositionRevision,
    assess_advisory_policy, guard_scope_advice,
};
use uuid::Uuid;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunScopeAdvisory {
    pub request_id: Uuid,
    pub candidate_set_id: Uuid,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
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
    pub(super) fn after_committed_start(
        started: &AdvisoryDispatchStart,
        authorization: &AdvisoryDispatchAuthorization,
        prepared: &PreparedScopeAdviceAttempt,
    ) -> Result<Self> {
        let dispatch = &started.dispatch;
        if !started.should_send
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
