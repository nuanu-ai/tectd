use super::*;
use crate::{
    FrozenMatrixRequirementsContext, MatrixRequirementsLocator,
    StoredMatrixRequirementsConfirmation, StoredMatrixRequirementsProposal,
};
use async_trait::async_trait;
use tect_domain::{
    DeclarationRecorder, DeclaredRequirementValue, EngineeringMode, MatrixRequirementsConfirmation,
    MatrixRequirementsProposal, MatrixRequirementsRevision, RequirementDeclarationPatch,
    RequirementsAnchor, resolve_matrix_requirements,
};

fn anchor() -> RequirementsAnchor {
    RequirementsAnchor::Program {
        program_id: Uuid::from_u128(1),
    }
}
fn revision(number: u64, mode: EngineeringMode) -> MatrixRequirementsRevision {
    let recorder = DeclarationRecorder {
        principal: "agent".into(),
        session: "session".into(),
    };
    let proposal = MatrixRequirementsProposal::new(
        anchor(),
        number,
        vec![RequirementDeclarationPatch::Set {
            value: DeclaredRequirementValue::Mode(mode),
        }],
        recorder.clone(),
    )
    .unwrap();
    let confirmation = MatrixRequirementsConfirmation::new(
        &proposal,
        number,
        proposal.digest().into(),
        "owner".into(),
        format!("owner-response-{number}"),
        recorder,
    )
    .unwrap();
    MatrixRequirementsRevision {
        proposal,
        confirmation: Some(confirmation),
    }
}

struct ContextStore {
    frozen: FrozenMatrixRequirementsContext,
    revisions: Vec<MatrixRequirementsRevision>,
    stale_work_revision: bool,
    locks: Vec<RequirementsAnchor>,
    freeze_saw_lock: bool,
}

#[async_trait]
impl MatrixRequirementsContextStore for ContextStore {
    async fn matrix_requirements_lineage(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: &MatrixRequirementsLocator,
        _: bool,
    ) -> Result<Vec<RequirementsAnchor>> {
        if self.stale_work_revision {
            Err(Error::StaleRevision)
        } else {
            Ok(vec![anchor()])
        }
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
        anchor: RequirementsAnchor,
    ) -> Result<u64> {
        self.locks.push(anchor);
        Ok(self.revisions.len() as u64)
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
        Err(Error::Forbidden)
    }
    async fn append_matrix_requirements_confirmation(
        &mut self,
        _: Uuid,
        _: &StoredMatrixRequirementsConfirmation,
    ) -> Result<()> {
        Err(Error::Forbidden)
    }
    async fn append_frozen_matrix_requirements(
        &mut self,
        _: Uuid,
        snapshot: &FrozenMatrixRequirementsContext,
    ) -> Result<FrozenMatrixRequirementsContext> {
        self.freeze_saw_lock = self.locks == vec![anchor()];
        Ok(snapshot.clone())
    }
    async fn frozen_matrix_requirements_by_id(
        &mut self,
        _: Uuid,
        snapshot_id: Uuid,
    ) -> Result<Option<FrozenMatrixRequirementsContext>> {
        Ok((self.frozen.id == snapshot_id).then(|| self.frozen.clone()))
    }
}

#[tokio::test]
async fn same_semantics_refresh_remains_current_but_source_revision_drift_does_not() {
    let first = revision(1, EngineeringMode::Mvp);
    let original = resolve_matrix_requirements(
        &[anchor()],
        std::slice::from_ref(&first),
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    let snapshot_id = Uuid::from_u128(2);
    let frozen = FrozenMatrixRequirementsContext {
        id: snapshot_id,
        anchor: anchor(),
        effective: original.clone(),
        payload_sha256: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&original).unwrap())
        ),
    };
    let binding = MatrixTaskRequirementsBinding {
        locator: MatrixRequirementsLocator::Program {
            program_id: Uuid::from_u128(1),
        },
        snapshot_id,
        semantic_digest: original.semantic_digest().into(),
        authority_schema: MATRIX_REQUIREMENTS_SCHEMA.into(),
    };
    let workspace = Uuid::new_v4();
    let principal = Uuid::new_v4();
    let mut store = ContextStore {
        frozen,
        revisions: vec![first, revision(2, EngineeringMode::Mvp)],
        stale_work_revision: false,
        locks: Vec::new(),
        freeze_saw_lock: false,
    };
    let resolved = lock_and_load_bound_matrix_context(&mut store, workspace, principal, &binding)
        .await
        .unwrap();
    assert_eq!(resolved.semantic_digest(), original.semantic_digest());
    assert_eq!(store.locks, vec![anchor()]);
    store
        .revisions
        .push(revision(3, EngineeringMode::Production));
    assert_eq!(
        lock_and_load_bound_matrix_context(&mut store, workspace, principal, &binding).await,
        Err(BoundContextFailure::CurrentStale),
    );
    store.stale_work_revision = true;
    assert_eq!(
        lock_and_load_bound_matrix_context(&mut store, workspace, principal, &binding).await,
        Err(BoundContextFailure::CurrentUnresolved),
    );
}

#[tokio::test]
async fn task_source_freeze_locks_context_before_snapshot_append() {
    let first = revision(1, EngineeringMode::Mvp);
    let effective = resolve_matrix_requirements(
        &[anchor()],
        std::slice::from_ref(&first),
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    let mut store = ContextStore {
        frozen: FrozenMatrixRequirementsContext {
            id: Uuid::from_u128(2),
            anchor: anchor(),
            effective: effective.clone(),
            payload_sha256: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&effective).unwrap())
            ),
        },
        revisions: vec![first],
        stale_work_revision: false,
        locks: Vec::new(),
        freeze_saw_lock: false,
    };
    let saved = crate::matrix_tasks::freeze_locked_matrix_requirements_context(
        &mut store,
        Uuid::new_v4(),
        Uuid::new_v4(),
        &MatrixRequirementsLocator::Program {
            program_id: Uuid::from_u128(1),
        },
    )
    .await
    .unwrap();
    assert!(store.freeze_saw_lock);
    assert_eq!(store.locks, vec![anchor()]);
    assert_eq!(
        saved.effective.semantic_digest(),
        effective.semantic_digest()
    );
}
