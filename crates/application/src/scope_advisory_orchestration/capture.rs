use super::{RunScopeAdvisory, ScopeAdvisoryOutcome};
use crate::WorkspaceService;
use tect_domain::{
    AdvisoryOpportunityState, AdvisoryPolicyInput, AdvisoryReason, RequestContext, Result,
    WorkspaceAdvisoryConfig, assess_advisory_policy,
};
use uuid::Uuid;

impl WorkspaceService {
    pub(super) async fn capture_early_scope_no_call(
        &self,
        context: &RequestContext,
        request: &RunScopeAdvisory,
        config: &WorkspaceAdvisoryConfig,
        actor: Uuid,
        session: Uuid,
        material_digest: String,
        reason: AdvisoryReason,
        revision: i64,
    ) -> Result<tect_domain::AdvisoryOpportunity> {
        let (mut tx, workspace, fresh_session) = self
            .scope_transaction(context, crate::TransactionMode::ReadWrite)
            .await?;
        if fresh_session.id != session
            || tx.advisory_config(workspace.id).await? != *config
            || tx
                .lock_candidate_revision(workspace.id, request.candidate_set_id)
                .await?
                .ok_or(tect_domain::Error::NotFound)?
                != revision
        {
            return Err(tect_domain::Error::StaleRevision);
        }
        let value = tx
            .capture_advisory_opportunity(
                workspace.id,
                &super::scope_opportunity_input(
                    request,
                    config,
                    actor,
                    session,
                    material_digest,
                    AdvisoryOpportunityState::NoCall,
                    reason,
                    Some(revision),
                ),
            )
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub(super) async fn capture_scope_opportunity(
        &self,
        context: &RequestContext,
        request: &RunScopeAdvisory,
        config: &WorkspaceAdvisoryConfig,
        actor: Uuid,
        session: Uuid,
        material_digest: String,
        state: AdvisoryOpportunityState,
        reason: AdvisoryReason,
        revision: Option<i64>,
    ) -> Result<tect_domain::AdvisoryOpportunity> {
        let (mut tx, workspace, _) = self
            .scope_transaction(context, crate::TransactionMode::ReadWrite)
            .await?;
        if tx.advisory_config(workspace.id).await? != *config {
            return Err(tect_domain::Error::StaleRevision);
        }
        let value = tx
            .capture_advisory_opportunity(
                workspace.id,
                &super::scope_opportunity_input(
                    request,
                    config,
                    actor,
                    session,
                    material_digest,
                    state,
                    reason,
                    revision,
                ),
            )
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub(super) async fn capture_invalid_scope_input(
        &self,
        context: &RequestContext,
        request: &RunScopeAdvisory,
        config: &WorkspaceAdvisoryConfig,
        actor: Uuid,
        session: Uuid,
    ) -> Result<ScopeAdvisoryOutcome> {
        let policy = assess_advisory_policy(AdvisoryPolicyInput {
            workspace_mode: config.mode,
            session_preference: request.session_preference,
            request_preference: request.request_preference,
            deterministic_input_valid: false,
            capability_available: true,
            provider_configured: config.provider_configured(),
        });
        let opportunity = self
            .capture_scope_opportunity(
                context,
                request,
                config,
                actor,
                session,
                super::no_call_digest(request, config.revision, policy.reason)?,
                policy.state,
                policy.reason,
                None,
            )
            .await?;
        Ok(ScopeAdvisoryOutcome {
            opportunity,
            advice: None,
        })
    }
}
