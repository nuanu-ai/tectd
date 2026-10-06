use crate::{
    ConfirmMatrixRequirementsContext, FrozenMatrixRequirementsContext,
    MatrixRequirementsContextStore, MatrixRequirementsLocator, ProposeMatrixRequirementsContext,
    StoredMatrixRequirementsConfirmation, StoredMatrixRequirementsProposal, TransactionMode,
    WorkspaceService,
};
use sha2::{Digest, Sha256};
use tect_domain::{
    DeclarationRecorder, EffectiveMatrixRequirements, Error, MATRIX_REQUIREMENTS_SCHEMA,
    MatrixRequirementsConfirmation, MatrixRequirementsProposal, RequestContext, RequirementsAnchor,
    Result, resolve_matrix_requirements,
};
use uuid::Uuid;

fn recorder(principal: Uuid, session: Uuid) -> DeclarationRecorder {
    DeclarationRecorder {
        principal: principal.to_string(),
        session: session.to_string(),
    }
}
fn anchor(lineage: &[RequirementsAnchor]) -> Result<RequirementsAnchor> {
    resolve_matrix_requirements(lineage, &[], MATRIX_REQUIREMENTS_SCHEMA)?;
    lineage.last().copied().ok_or(Error::Forbidden)
}
async fn effective(
    store: &mut dyn MatrixRequirementsContextStore,
    workspace_id: Uuid,
    lineage: &[RequirementsAnchor],
) -> Result<EffectiveMatrixRequirements> {
    let revisions = store
        .matrix_requirements_revisions(workspace_id, lineage)
        .await?;
    resolve_matrix_requirements(lineage, &revisions, MATRIX_REQUIREMENTS_SCHEMA)
}

impl WorkspaceService {
    pub async fn propose_matrix_requirements_context(
        &self,
        context: &RequestContext,
        request: &ProposeMatrixRequirementsContext,
    ) -> Result<StoredMatrixRequirementsProposal> {
        if request.request_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        if tx.session_principal(session.id).await? != identity.principal_id {
            return Err(Error::Forbidden);
        }
        let store = tx
            .matrix_requirements_context_store()
            .ok_or(Error::Forbidden)?;
        let lineage = store
            .matrix_requirements_lineage(
                workspace.id,
                identity.principal_id,
                &request.locator,
                true,
            )
            .await?;
        let target = anchor(&lineage)?;
        if let Some(existing) = store
            .matrix_requirements_proposal_by_request(workspace.id, request.request_id)
            .await?
        {
            if existing.request != *request
                || existing.proposal.anchor() != target
                || existing.proposal.recorder() != &recorder(identity.principal_id, session.id)
            {
                return Err(Error::InputConflict);
            }
            // Rehydrated proposal must still have its canonical digest.
            let rebuilt = MatrixRequirementsProposal::new(
                existing.proposal.anchor(),
                existing.proposal.revision(),
                existing.proposal.patches().to_vec(),
                existing.proposal.recorder().clone(),
            )?;
            if existing.proposal != rebuilt {
                return Err(Error::InputConflict);
            }
            tx.commit().await?;
            return Ok(existing);
        }
        if store
            .lock_matrix_requirements_head(workspace.id, target)
            .await?
            != request.expected_context_revision
        {
            return Err(Error::StaleRevision);
        }
        let revision = request
            .expected_context_revision
            .checked_add(1)
            .ok_or(Error::InvalidArguments)?;
        let record = StoredMatrixRequirementsProposal {
            request: request.clone(),
            proposal: MatrixRequirementsProposal::new(
                target,
                revision,
                request.patches.clone(),
                recorder(identity.principal_id, session.id),
            )?,
            recorded_at_epoch_seconds: crate::matrix_verification::current_epoch_seconds()?,
        };
        store
            .append_matrix_requirements_proposal(
                workspace.id,
                request.expected_context_revision,
                &record,
            )
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    /// The owner must actually respond to the exact proposal before invoking
    /// this command. The opaque response reference records that workflow; this
    /// service proves authorization and binding, not human authorship.
    pub async fn confirm_matrix_requirements_context(
        &self,
        context: &RequestContext,
        request: &ConfirmMatrixRequirementsContext,
    ) -> Result<StoredMatrixRequirementsConfirmation> {
        if request.request_id.is_nil()
            || request.proposal_revision == 0
            || request.owner_response_ref.trim().is_empty()
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        if tx.session_principal(session.id).await? != identity.principal_id {
            return Err(Error::Forbidden);
        }
        let store = tx
            .matrix_requirements_context_store()
            .ok_or(Error::Forbidden)?;
        let lineage = store
            .matrix_requirements_lineage(
                workspace.id,
                identity.principal_id,
                &request.locator,
                true,
            )
            .await?;
        let target = anchor(&lineage)?;
        if let Some(existing) = store
            .matrix_requirements_confirmation_by_request(workspace.id, request.request_id)
            .await?
        {
            if existing.request != *request
                || existing.confirmation.owner_principal() != identity.principal_id.to_string()
                || existing.confirmation.recorder() != &recorder(identity.principal_id, session.id)
            {
                return Err(Error::InputConflict);
            }
            let revisions = store
                .matrix_requirements_revisions(workspace.id, &[target])
                .await?;
            let proposal = revisions
                .iter()
                .find(|r| r.proposal.revision() == request.proposal_revision)
                .ok_or(Error::NotFound)?;
            existing.confirmation.validate_for(&proposal.proposal)?;
            tx.commit().await?;
            return Ok(existing);
        }
        if store
            .lock_matrix_requirements_head(workspace.id, target)
            .await?
            != request.proposal_revision
        {
            return Err(Error::StaleRevision);
        }
        let revisions = store
            .matrix_requirements_revisions(workspace.id, &[target])
            .await?;
        let proposal = revisions
            .iter()
            .find(|r| {
                r.proposal.anchor() == target && r.proposal.revision() == request.proposal_revision
            })
            .ok_or(Error::NotFound)?;
        if proposal.confirmation.is_some() {
            return Err(Error::InputConflict);
        }
        let record = StoredMatrixRequirementsConfirmation {
            request: request.clone(),
            confirmation: MatrixRequirementsConfirmation::new(
                &proposal.proposal,
                request.proposal_revision,
                request.proposal_digest.clone(),
                identity.principal_id.to_string(),
                request.owner_response_ref.clone(),
                recorder(identity.principal_id, session.id),
            )?,
            recorded_at_epoch_seconds: crate::matrix_verification::current_epoch_seconds()?,
        };
        store
            .append_matrix_requirements_confirmation(workspace.id, &record)
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    pub async fn get_effective_matrix_requirements_context(
        &self,
        context: &RequestContext,
        locator: &MatrixRequirementsLocator,
    ) -> Result<EffectiveMatrixRequirements> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let store = tx
            .matrix_requirements_context_store()
            .ok_or(Error::Forbidden)?;
        let lineage = store
            .matrix_requirements_lineage(workspace.id, identity.principal_id, locator, false)
            .await?;
        let value = effective(store, workspace.id, &lineage).await?;
        tx.commit().await?;
        Ok(value)
    }
}

/// Called only when a material consumer prepares within its existing write
/// transaction. No independent transaction or automatic execution occurs.
pub async fn freeze_effective_matrix_requirements_context(
    store: &mut dyn MatrixRequirementsContextStore,
    workspace_id: Uuid,
    principal_id: Uuid,
    locator: &MatrixRequirementsLocator,
) -> Result<FrozenMatrixRequirementsContext> {
    let lineage = store
        .matrix_requirements_lineage(workspace_id, principal_id, locator, true)
        .await?;
    let value = effective(store, workspace_id, &lineage).await?;
    let bytes = serde_json::to_vec(&value).map_err(|_| Error::InvalidArguments)?;
    let snapshot = FrozenMatrixRequirementsContext {
        id: Uuid::new_v4(),
        anchor: anchor(&lineage)?,
        effective: value,
        payload_sha256: format!("{:x}", Sha256::digest(bytes)),
    };
    store
        .append_frozen_matrix_requirements(workspace_id, &snapshot)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use tect_domain::{
        DeclaredRequirementValue, EngineeringMode, MatrixRequirementsRevision,
        RequirementDeclarationPatch,
    };

    struct MemoryContext {
        lineage: Vec<RequirementsAnchor>,
        revisions: Vec<MatrixRequirementsRevision>,
        frozen: Option<FrozenMatrixRequirementsContext>,
        writes: usize,
    }
    #[async_trait]
    impl MatrixRequirementsContextStore for MemoryContext {
        async fn matrix_requirements_lineage(
            &mut self,
            _: Uuid,
            _: Uuid,
            _: &MatrixRequirementsLocator,
            _: bool,
        ) -> Result<Vec<RequirementsAnchor>> {
            Ok(self.lineage.clone())
        }
        async fn matrix_requirements_proposal_by_request(
            &mut self,
            _: Uuid,
            _: Uuid,
        ) -> Result<Option<StoredMatrixRequirementsProposal>> {
            Ok(None)
        }
        async fn matrix_requirements_confirmation_by_request(
            &mut self,
            _: Uuid,
            _: Uuid,
        ) -> Result<Option<StoredMatrixRequirementsConfirmation>> {
            Ok(None)
        }
        async fn lock_matrix_requirements_head(
            &mut self,
            _: Uuid,
            _: RequirementsAnchor,
        ) -> Result<u64> {
            Ok(1)
        }
        async fn matrix_requirements_revisions(
            &mut self,
            _: Uuid,
            _: &[RequirementsAnchor],
        ) -> Result<Vec<MatrixRequirementsRevision>> {
            Ok(self.revisions.clone())
        }
        async fn append_matrix_requirements_proposal(
            &mut self,
            _: Uuid,
            _: u64,
            _: &StoredMatrixRequirementsProposal,
        ) -> Result<()> {
            self.writes += 1;
            Ok(())
        }
        async fn append_matrix_requirements_confirmation(
            &mut self,
            _: Uuid,
            _: &StoredMatrixRequirementsConfirmation,
        ) -> Result<()> {
            self.writes += 1;
            Ok(())
        }
        async fn append_frozen_matrix_requirements(
            &mut self,
            _: Uuid,
            s: &FrozenMatrixRequirementsContext,
        ) -> Result<FrozenMatrixRequirementsContext> {
            if let Some(old) = &self.frozen
                && old.payload_sha256 == s.payload_sha256
            {
                return Ok(old.clone());
            }
            self.writes += 1;
            self.frozen = Some(s.clone());
            Ok(s.clone())
        }
    }
    fn fixture() -> MemoryContext {
        let program_id = Uuid::new_v4();
        let anchor = RequirementsAnchor::Program { program_id };
        let proposal = MatrixRequirementsProposal::new(
            anchor,
            1,
            vec![RequirementDeclarationPatch::Set {
                value: DeclaredRequirementValue::Mode(EngineeringMode::Mvp),
            }],
            recorder(Uuid::new_v4(), Uuid::new_v4()),
        )
        .unwrap();
        MemoryContext {
            lineage: vec![anchor],
            revisions: vec![MatrixRequirementsRevision {
                proposal,
                confirmation: None,
            }],
            frozen: None,
            writes: 0,
        }
    }
    #[tokio::test]
    async fn pending_read_is_empty_and_never_appends() {
        let mut store = fixture();
        let workspace = Uuid::new_v4();
        let lineage = store.lineage.clone();
        let value = effective(&mut store, workspace, &lineage).await.unwrap();
        assert_eq!(
            serde_json::to_value(value).unwrap()["values"],
            serde_json::json!({})
        );
        assert_eq!(store.writes, 0);
    }
    #[tokio::test]
    async fn freeze_returns_persisted_identity_on_full_payload_replay() {
        let mut store = fixture();
        let anchor = store.lineage[0];
        let proposal = &store.revisions[0].proposal;
        store.revisions[0].confirmation = Some(
            MatrixRequirementsConfirmation::new(
                proposal,
                1,
                proposal.digest().to_owned(),
                Uuid::new_v4().to_string(),
                "controlled-fixture:explicit-response".into(),
                recorder(Uuid::new_v4(), Uuid::new_v4()),
            )
            .unwrap(),
        );
        let locator = MatrixRequirementsLocator::Program {
            program_id: anchor.program_id(),
        };
        let workspace = Uuid::new_v4();
        let principal = Uuid::new_v4();
        let first = freeze_effective_matrix_requirements_context(
            &mut store, workspace, principal, &locator,
        )
        .await
        .unwrap();
        let second = freeze_effective_matrix_requirements_context(
            &mut store, workspace, principal, &locator,
        )
        .await
        .unwrap();
        assert_eq!(first, second);
        assert_eq!(store.writes, 1);
    }
}
