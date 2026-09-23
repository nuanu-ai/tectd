use super::{DecideScopeAdvisory, PreserveScopeAdvisory};
use crate::{
    ScopeAuthorityRequest, ScopeDispositionRecord, ScopePreservationReceiptInput,
    Sha256ScopeDigest, TransactionMode, WorkspaceService,
};
use tect_domain::{
    FreshScopeObservation, RequestContext, Result, ScopeDispositionRevision,
    ScopePreservationResult, evaluate_scope_preservation,
};
use uuid::Uuid;

impl WorkspaceService {
    pub(crate) async fn decide_scope_advisory(
        &self,
        context: &RequestContext,
        input: DecideScopeAdvisory,
    ) -> Result<ScopeDispositionRevision> {
        let (mut tx, workspace, session) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let actor = tx.session_principal(session.id).await?;
        let result = tx
            .cas_scope_advisory_disposition(
                workspace.id,
                ScopeDispositionRecord {
                    opportunity_id: input.opportunity_id,
                    candidate_set_id: input.candidate_set_id,
                    actor_id: actor,
                    session_id: session.id,
                    request: input.request,
                },
            )
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub(crate) async fn preserve_scope_advisory(
        &self,
        context: &RequestContext,
        input: PreserveScopeAdvisory,
    ) -> Result<(Uuid, ScopePreservationResult)> {
        let (mut read, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, session) = Self::bound_session(&mut *read, context, &identity).await?;
        let actor = read.session_principal(session.id).await?;
        read.commit().await?;
        let authority_request = ScopeAuthorityRequest {
            tenant_id: identity.tenant_id,
            workspace_id: workspace.id,
            actor_id: actor,
            session_id: session.id,
            candidate_set_id: input.candidate_set_id,
        };
        let current = match self.scope_authority.observe(&authority_request).await? {
            crate::ScopeAuthorityOutcome::Authorized(value) => value,
            crate::ScopeAuthorityOutcome::AuthorizedInvalid(_) => {
                return Err(tect_domain::Error::InvalidArguments);
            }
        };
        super::validate_observation(&authority_request, &current)?;
        let current_manifest = self.scope_manifest_supplier.supply(&current).await?;
        let candidate_set_revision = current.source.candidate_set_revision;
        let observation = FreshScopeObservation {
            source: current.source,
            manifest: current_manifest,
            candidate_set_revision,
            advice_id: input.advice.id.clone(),
        };
        let result = evaluate_scope_preservation(
            &Sha256ScopeDigest,
            &input.manifest,
            &input.advice,
            &input.disposition,
            &observation,
        )?;
        let (mut tx, workspace, _) = self
            .scope_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let id = tx
            .persist_scope_preservation_receipt(
                workspace.id,
                &ScopePreservationReceiptInput {
                    receipt_id: input.receipt_id,
                    request_id: input.request_id,
                    opportunity_id: input.opportunity_id,
                    candidate_set_id: input.candidate_set_id,
                    disposition_id: input.disposition.id,
                    observation,
                    result: result.clone(),
                },
            )
            .await?;
        tx.commit().await?;
        Ok((id, result))
    }
}
