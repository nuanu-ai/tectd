#[cfg(test)]
use crate::AdvisoryLifecycleCapability;
#[cfg(test)]
use crate::advisory_ports::{
    AdvisoryProviderObservation, AdvisoryProviderRequest, ControlledAdvisoryDispatch,
    ControlledAdvisoryResult,
};
use crate::{TransactionMode, UnitOfWork, WorkspaceService};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_domain::{
    ADVISORY_DECISION_POINT_VERSION, AdvisoryAuditPage, AdvisoryAuditQuery, AdvisoryCapability,
    AdvisoryDecisionPoint, AdvisoryOpportunityDetail, AdvisoryOpportunityInput,
    CandidateScopeAdvisoryProjection, ConfigureWorkspaceAdvisory, Error, PrincipalRole, Result,
    SelectedSaveObservation, SelectedSaveObservationRequest, WorkspaceAdvisoryConfig,
};
use uuid::Uuid;

/// Public verifier input. Identity and session are bound to the host credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySelectedSave {
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_link_id: Uuid,
    pub caller_receipt_request_id: Uuid,
    pub target_revision: i64,
}
#[cfg(test)]
use tect_domain::{
    AdvisoryDispatchAuthorization, AdvisoryDispatchSeal, AdvisoryDispatchState,
    AdvisoryOpportunityState, AdvisoryReconciliationEvidence, AdvisoryRetryBasis,
};

#[derive(Debug, Clone, Copy, Default)]
pub struct Sha256ScopeDigest;

impl tect_domain::ScopeDigest for Sha256ScopeDigest {
    fn sha256(&self, domain: &'static str, canonical_bytes: &[u8]) -> String {
        let mut digest = Sha256::new();
        digest.update(domain.as_bytes());
        digest.update([0]);
        digest.update(canonical_bytes);
        format!("{:x}", digest.finalize())
    }
}

impl WorkspaceService {
    async fn verifier_candidate_transaction(
        &self,
        context: &tect_domain::RequestContext,
        mode: TransactionMode,
    ) -> Result<(
        Box<dyn UnitOfWork>,
        tect_domain::Workspace,
        tect_domain::Session,
    )> {
        let (mut tx, identity) = self.authenticated(context, mode).await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        if mode == TransactionMode::ReadWrite {
            tx.lock_native_session(identity.host_id, &context.native_session_id)
                .await?;
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        Ok((tx, workspace, session))
    }

    pub async fn verify_selected_save(
        &self,
        context: &tect_domain::RequestContext,
        request: &VerifySelectedSave,
    ) -> Result<SelectedSaveObservation> {
        let (mut tx, workspace, session) = self
            .verifier_candidate_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let bound = SelectedSaveObservationRequest {
            request_id: request.request_id,
            opportunity_id: request.opportunity_id,
            candidate_set_id: request.candidate_set_id,
            caller_link_id: request.caller_link_id,
            caller_receipt_request_id: request.caller_receipt_request_id,
            target_revision: request.target_revision,
            session_id: session.id,
        };
        if !bound.valid() {
            return Err(Error::InvalidArguments);
        }
        let observed = tx
            .independently_observe_selected_scope_save(workspace.id, &bound)
            .await?;
        tx.commit().await?;
        Ok(observed)
    }

    async fn candidate_read_transaction(
        &self,
        context: &tect_domain::RequestContext,
    ) -> Result<(Box<dyn UnitOfWork>, tect_domain::Workspace, PrincipalRole)> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if !matches!(
            identity.role,
            PrincipalRole::Owner | PrincipalRole::Verifier
        ) {
            return Err(Error::Forbidden);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        Ok((tx, workspace, identity.role))
    }

    /// Authenticate a malformed candidate advisory call without granting the
    /// verifier any of the general workspace state surface.
    pub async fn authenticate_candidate_advisory_session(
        &self,
        context: &tect_domain::RequestContext,
    ) -> Result<()> {
        let (tx, _, _) = self.candidate_read_transaction(context).await?;
        tx.commit().await
    }
    async fn advisory_transaction(
        &self,
        context: &tect_domain::RequestContext,
        mode: TransactionMode,
    ) -> Result<(
        Box<dyn UnitOfWork>,
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

    pub async fn advisory_config(
        &self,
        context: &tect_domain::RequestContext,
    ) -> Result<WorkspaceAdvisoryConfig> {
        let (mut tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let config = tx.advisory_config(workspace.id).await?;
        tx.commit().await?;
        Ok(config)
    }

    pub async fn configure_advisory(
        &self,
        context: &tect_domain::RequestContext,
        request: &ConfigureWorkspaceAdvisory,
    ) -> Result<WorkspaceAdvisoryConfig> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .advisory_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal = tx.session_principal(session.id).await?;
        let config = tx
            .configure_advisory(workspace.id, principal, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(config)
    }

    pub async fn advisory_audit(
        &self,
        context: &tect_domain::RequestContext,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage> {
        query.validate()?;
        let (mut tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadOnly)
            .await?;
        if let Some(scope_id) = query.scope_id {
            tx.native_scope(workspace.id, scope_id)
                .await?
                .ok_or(tect_domain::Error::NotFound)?;
        }
        let page = tx
            .advisory_audit(workspace.id, query.scope_id, query)
            .await?;
        tx.commit().await?;
        Ok(page)
    }

    pub async fn scope_advisory_audit(
        &self,
        context: &tect_domain::RequestContext,
        scope_id: uuid::Uuid,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage> {
        query.validate()?;
        if scope_id.is_nil() {
            return Err(tect_domain::Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadOnly)
            .await?;
        tx.native_scope(workspace.id, scope_id)
            .await?
            .ok_or(tect_domain::Error::NotFound)?;
        let page = tx
            .advisory_audit(workspace.id, Some(scope_id), query)
            .await?;
        tx.commit().await?;
        Ok(page)
    }

    pub async fn scope_advisory_get(
        &self,
        context: &tect_domain::RequestContext,
        scope_id: uuid::Uuid,
        opportunity_id: uuid::Uuid,
    ) -> Result<AdvisoryOpportunityDetail> {
        if scope_id.is_nil() || opportunity_id.is_nil() {
            return Err(tect_domain::Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .advisory_transaction(context, TransactionMode::ReadOnly)
            .await?;
        tx.native_scope(workspace.id, scope_id)
            .await?
            .ok_or(tect_domain::Error::NotFound)?;
        let detail = tx
            .advisory_opportunity_detail(workspace.id, scope_id, opportunity_id)
            .await?;
        tx.commit().await?;
        Ok(detail)
    }

    pub async fn candidate_advisory_audit(
        &self,
        context: &tect_domain::RequestContext,
        candidate_set_id: uuid::Uuid,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage> {
        query.validate()?;
        if candidate_set_id.is_nil() || query.scope_id.is_some() {
            return Err(tect_domain::Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self.candidate_read_transaction(context).await?;
        if !tx
            .advisory_candidate_set_exists(workspace.id, candidate_set_id)
            .await?
        {
            return Err(tect_domain::Error::NotFound);
        }
        let page = tx
            .candidate_advisory_audit(workspace.id, candidate_set_id, query)
            .await?;
        tx.commit().await?;
        Ok(page)
    }

    pub async fn candidate_advisory_get(
        &self,
        context: &tect_domain::RequestContext,
        candidate_set_id: uuid::Uuid,
        opportunity_id: uuid::Uuid,
    ) -> Result<AdvisoryOpportunityDetail> {
        if candidate_set_id.is_nil() || opportunity_id.is_nil() {
            return Err(tect_domain::Error::InvalidArguments);
        }
        let (mut tx, workspace, role) = self.candidate_read_transaction(context).await?;
        if !tx
            .advisory_candidate_set_exists(workspace.id, candidate_set_id)
            .await?
        {
            return Err(tect_domain::Error::NotFound);
        }
        let mut detail = tx
            .candidate_advisory_opportunity_detail(workspace.id, candidate_set_id, opportunity_id)
            .await?;
        if role == PrincipalRole::Owner {
            if let Some(advice) = tx
                .guarded_scope_advice(workspace.id, opportunity_id)
                .await?
            {
                let manifest = tx
                    .scope_advisory_manifest(workspace.id, opportunity_id)
                    .await?
                    .ok_or(Error::StorageUnavailable)?;
                if manifest.source.candidate_set_id != candidate_set_id
                    || advice.opportunity_id.is_some_and(|id| id != opportunity_id)
                    || advice.source_digest != manifest.source.digest
                    || advice.manifest_digest != manifest.whole_set_digest
                    || advice.eligible_set_digest != manifest.eligible_set_digest
                {
                    return Err(Error::StorageUnavailable);
                }
                detail.scope_decomposition = Some(CandidateScopeAdvisoryProjection {
                    version: 1,
                    manifest,
                    advice,
                });
            }
        }
        tx.commit().await?;
        Ok(detail)
    }
}

#[cfg(test)]
#[allow(dead_code)]
fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
async fn attempt_provider_once(
    provider: &dyn crate::AdvisoryProvider,
    request: &AdvisoryProviderRequest,
) -> Result<AdvisoryProviderObservation> {
    provider.attempt(request).await
}

pub(crate) fn scope_decomposition_opportunity(
    session_id: uuid::Uuid,
    authorized_actor_id: uuid::Uuid,
    request: &tect_domain::BeginCandidateSet,
    config: &WorkspaceAdvisoryConfig,
    session_preference: tect_domain::AdvisoryRequestPreference,
    deterministic_input_valid: bool,
) -> AdvisoryOpportunityInput {
    let canonical_material = serde_json::to_vec(&(
        request,
        config.revision,
        config.mode.as_str(),
        session_preference.as_str(),
        deterministic_input_valid,
        tect_domain::ADVISORY_POLICY_VERSION,
    ))
    .expect("advisory material contains only serializable domain values");
    let material_digest = format!("{:x}", Sha256::digest(&canonical_material));
    let decision = tect_domain::assess_advisory_policy(tect_domain::AdvisoryPolicyInput {
        workspace_mode: config.mode,
        session_preference,
        request_preference: request.advisory_preference,
        deterministic_input_valid,
        capability_available: false,
        provider_configured: config.provider_configured(),
    });
    AdvisoryOpportunityInput {
        session_id,
        authorized_actor_id,
        capability: AdvisoryCapability::ScopeDecomposition,
        decision_point: AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
        decision_point_version: ADVISORY_DECISION_POINT_VERSION,
        workflow_occurrence_key: request.request_id.to_string(),
        target_kind: "program".into(),
        target_id: Some(request.program_id),
        work_revision: Some(request.program_revision),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        source_ref: None,
        session_preference,
        request_preference: request.advisory_preference,
        config_revision: config.revision,
        material_digest,
        state: decision.state,
        primary_reason: decision.reason,
    }
}

#[async_trait]
pub(crate) trait DurableOpportunityBoundary: Send {
    async fn capture(
        &mut self,
        workspace_id: uuid::Uuid,
        input: &AdvisoryOpportunityInput,
    ) -> Result<tect_domain::AdvisoryOpportunity>;
    async fn commit_boundary(self) -> Result<()>;
}

#[async_trait]
impl DurableOpportunityBoundary for Box<dyn UnitOfWork> {
    async fn capture(
        &mut self,
        workspace_id: uuid::Uuid,
        input: &AdvisoryOpportunityInput,
    ) -> Result<tect_domain::AdvisoryOpportunity> {
        self.capture_advisory_opportunity(workspace_id, input).await
    }

    async fn commit_boundary(self) -> Result<()> {
        self.commit().await
    }
}

pub(crate) async fn commit_scope_opportunity<B: DurableOpportunityBoundary>(
    mut boundary: B,
    workspace_id: uuid::Uuid,
    input: &AdvisoryOpportunityInput,
) -> Result<tect_domain::AdvisoryOpportunity> {
    let opportunity = boundary.capture(workspace_id, input).await?;
    boundary.commit_boundary().await?;
    Ok(opportunity)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod controlled_dispatch;
