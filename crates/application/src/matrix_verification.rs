use crate::{
    ContextMatrixVerificationStore, MatrixEvidenceValidator, MatrixRequirementsContextStore,
    MatrixTaskRequirementsBinding, MatrixTaskRevision, MatrixVerificationStore, TransactionMode,
    WorkspaceService,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};
use tect_domain::{
    CONTEXT_MATRIX_VERIFICATION_SCHEMA, ContextMatrixVerificationRecord,
    EffectiveMatrixRequirements, Error, EvidenceValidationOutcome, MATRIX_REQUIREMENTS_SCHEMA,
    MATRIX_VERIFICATION_SCHEMA, MatrixVerificationRecord, PrincipalRole, RequestContext, Result,
    evaluate_context_matrix_verification, evaluate_matrix_verification, matrix_input_digest,
    required_matrix_facts, required_matrix_operating_facts, resolve_matrix_requirements,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixEvidenceReference {
    pub fact_path: String,
    pub evidence_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyMatrixTask {
    pub task_id: Uuid,
    pub expected_revision: i64,
    /// Digest returned with the saved MatrixTaskRevision.
    pub input_digest: String,
    pub evidence: Vec<MatrixEvidenceReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifiedMatrixTask {
    Legacy(MatrixVerificationRecord),
    Context(ContextMatrixVerificationRecord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundContextFailure {
    SnapshotMissing,
    BindingMismatch,
    CurrentUnresolved,
    CurrentStale,
    AuthoritySchemaUnsupported,
}

/// The saved snapshot is provenance; current effective declarations are a
/// separate applicability check. Never replace the original snapshot ID.
pub(crate) async fn load_bound_matrix_context(
    store: &mut dyn MatrixRequirementsContextStore,
    workspace_id: Uuid,
    principal_id: Uuid,
    binding: &MatrixTaskRequirementsBinding,
) -> std::result::Result<EffectiveMatrixRequirements, BoundContextFailure> {
    if binding.authority_schema != MATRIX_REQUIREMENTS_SCHEMA {
        return Err(BoundContextFailure::AuthoritySchemaUnsupported);
    }
    let frozen = store
        .frozen_matrix_requirements_by_id(workspace_id, binding.snapshot_id)
        .await
        .map_err(|_| BoundContextFailure::SnapshotMissing)?
        .ok_or(BoundContextFailure::SnapshotMissing)?;
    let bytes =
        serde_json::to_vec(&frozen.effective).map_err(|_| BoundContextFailure::BindingMismatch)?;
    let payload_sha256 = format!("{:x}", Sha256::digest(bytes));
    if frozen.id != binding.snapshot_id
        || frozen.payload_sha256 != payload_sha256
        || frozen.effective.schema() != binding.authority_schema
        || frozen.effective.semantic_digest() != binding.semantic_digest
    {
        return Err(BoundContextFailure::BindingMismatch);
    }
    let lineage = store
        .matrix_requirements_lineage(workspace_id, principal_id, &binding.locator, false)
        .await
        .map_err(|_| BoundContextFailure::CurrentUnresolved)?;
    if lineage.last().copied() != Some(frozen.anchor) {
        return Err(BoundContextFailure::BindingMismatch);
    }
    let revisions = store
        .matrix_requirements_revisions(workspace_id, &lineage)
        .await
        .map_err(|_| BoundContextFailure::CurrentUnresolved)?;
    let current = resolve_matrix_requirements(&lineage, &revisions, MATRIX_REQUIREMENTS_SCHEMA)
        .map_err(|_| BoundContextFailure::CurrentUnresolved)?;
    if current.schema() != binding.authority_schema {
        return Err(BoundContextFailure::AuthoritySchemaUnsupported);
    }
    if current.semantic_digest() != binding.semantic_digest {
        return Err(BoundContextFailure::CurrentStale);
    }
    Ok(frozen.effective)
}

/// Hold Program -> Scope -> Slice declaration locks through the caller's
/// authorization or send-start commit. Proposal and confirmation writes use
/// these same transaction-scoped locks.
pub(crate) async fn lock_and_load_bound_matrix_context(
    store: &mut dyn MatrixRequirementsContextStore,
    workspace_id: Uuid,
    principal_id: Uuid,
    binding: &MatrixTaskRequirementsBinding,
) -> std::result::Result<EffectiveMatrixRequirements, BoundContextFailure> {
    let lineage = store
        .matrix_requirements_lineage(workspace_id, principal_id, &binding.locator, false)
        .await
        .map_err(|_| BoundContextFailure::CurrentUnresolved)?;
    for anchor in lineage {
        store
            .lock_matrix_requirements_head(workspace_id, anchor)
            .await
            .map_err(|_| BoundContextFailure::CurrentUnresolved)?;
    }
    load_bound_matrix_context(store, workspace_id, principal_id, binding).await
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MatrixVerificationActor {
    pub workspace_id: Uuid,
    pub verifier_principal_id: Uuid,
    pub verifier_session_id: Uuid,
}

impl WorkspaceService {
    /// Authorize a malformed verifier call against the same active, bound
    /// session required by a valid verification, without opening task data.
    pub async fn authenticate_matrix_verifier_session(
        &self,
        context: &RequestContext,
    ) -> Result<()> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        tx.commit().await
    }

    /// Verifies a saved owner revision. No JEV or advisory call is made.
    pub async fn verify_matrix_task(
        &self,
        context: &RequestContext,
        request: &VerifyMatrixTask,
    ) -> Result<VerifiedMatrixTask> {
        if request.task_id.is_nil()
            || request.expected_revision < 1
            || request.input_digest.len() != 64
            || !request.input_digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let source = tx
            .matrix_task_source(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        let bound_context = if let Some(binding) = source.requirements_binding.as_ref() {
            Some(
                lock_and_load_bound_matrix_context(
                    tx.matrix_requirements_context_store()
                        .ok_or(Error::StorageUnavailable)?,
                    workspace.id,
                    identity.principal_id,
                    binding,
                )
                .await
                .map_err(|_| Error::StaleRevision)?,
            )
        } else {
            None
        };
        let locked = tx
            .lock_matrix_task(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if source.revision != locked {
            return Err(Error::StaleRevision);
        }
        let actor = MatrixVerificationActor {
            workspace_id: workspace.id,
            verifier_principal_id: identity.principal_id,
            verifier_session_id: session.id,
        };
        let record = if let Some(binding) = source.requirements_binding.as_ref() {
            let context = bound_context.as_ref().ok_or(Error::InternalInvariant)?;
            let store = tx
                .context_matrix_verification_store()
                .ok_or(Error::StorageUnavailable)?;
            VerifiedMatrixTask::Context(
                verify_locked_context_revision(
                    store,
                    self.matrix_evidence_validator.as_ref(),
                    actor,
                    &locked,
                    binding.snapshot_id,
                    context,
                    request,
                    &current_epoch_seconds,
                )
                .await?,
            )
        } else {
            let store = tx
                .matrix_verification_store()
                .ok_or(Error::StorageUnavailable)?;
            VerifiedMatrixTask::Legacy(
                verify_locked_revision(
                    store,
                    self.matrix_evidence_validator.as_ref(),
                    actor,
                    &locked,
                    request,
                    &current_epoch_seconds,
                )
                .await?,
            )
        };
        tx.commit().await?;
        Ok(record)
    }
}

pub(crate) async fn verify_locked_revision(
    store: &mut dyn MatrixVerificationStore,
    validator: &dyn MatrixEvidenceValidator,
    actor: MatrixVerificationActor,
    revision: &MatrixTaskRevision,
    request: &VerifyMatrixTask,
    clock: &(dyn Fn() -> Result<i64> + Sync),
) -> Result<MatrixVerificationRecord> {
    let MatrixVerificationActor {
        workspace_id,
        verifier_principal_id,
        verifier_session_id,
    } = actor;
    if revision.task_id != request.task_id {
        return Err(Error::NotFound);
    }
    if revision.revision != request.expected_revision {
        return Err(Error::StaleRevision);
    }
    if revision.input_digest != request.input_digest {
        return Err(Error::InputConflict);
    }
    if verifier_principal_id == revision.recorded_by_principal_id {
        return Err(Error::Forbidden);
    }
    let required = required_matrix_facts(&revision.input)?;
    if request.evidence.len() != required.len() {
        return Err(Error::InvalidArguments);
    }
    let mut refs = BTreeMap::new();
    for evidence in &request.evidence {
        if evidence.evidence_ref.trim().is_empty()
            || evidence.evidence_ref.len() > 4096
            || refs
                .insert(evidence.fact_path.as_str(), evidence.evidence_ref.as_str())
                .is_some()
        {
            return Err(Error::InvalidArguments);
        }
    }
    let mut bindings = Vec::with_capacity(required.len());
    let validation_now = clock()?;
    for fact in &required {
        let evidence_ref = refs
            .get(fact.path.as_str())
            .ok_or(Error::InvalidArguments)?;
        let binding = validator
            .validate(
                workspace_id,
                revision.task_id,
                revision.revision,
                fact,
                evidence_ref,
                validation_now,
            )
            .await?;
        if binding.fact_path != fact.path
            || binding.value_digest != fact.value_digest
            || binding.evidence_ref != *evidence_ref
            || binding.validation_outcome != EvidenceValidationOutcome::Accepted
        {
            return Err(Error::InvalidArguments);
        }
        bindings.push(binding);
    }
    let mut record = MatrixVerificationRecord {
        schema: MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: revision.task_id.to_string(),
        task_revision: revision.revision.to_string(),
        input_digest: matrix_input_digest(&revision.input)?,
        owner_principal: revision.recorded_by_principal_id.to_string(),
        verifier_principal: verifier_principal_id.to_string(),
        policy_version: validator.policy_version().into(),
        bindings,
        digest: String::new(),
    };
    if record.input_digest != revision.input_digest {
        return Err(Error::InternalInvariant);
    }
    record.digest = record.canonical_digest()?;
    evaluate_matrix_verification(
        &record.task_id,
        &record.task_revision,
        &revision.input,
        &record,
        clock()?,
    )?;
    store
        .append_matrix_verification(
            workspace_id,
            verifier_session_id,
            revision.task_id,
            revision.revision,
            &revision.input_digest,
            &record,
        )
        .await?;
    Ok(record)
}

pub(crate) async fn verify_locked_context_revision(
    store: &mut dyn ContextMatrixVerificationStore,
    validator: &dyn MatrixEvidenceValidator,
    actor: MatrixVerificationActor,
    revision: &MatrixTaskRevision,
    frozen_snapshot_id: Uuid,
    context: &EffectiveMatrixRequirements,
    request: &VerifyMatrixTask,
    clock: &(dyn Fn() -> Result<i64> + Sync),
) -> Result<ContextMatrixVerificationRecord> {
    if revision.task_id != request.task_id {
        return Err(Error::NotFound);
    }
    if revision.revision != request.expected_revision {
        return Err(Error::StaleRevision);
    }
    if revision.input_digest != request.input_digest {
        return Err(Error::InputConflict);
    }
    if actor.verifier_principal_id == revision.recorded_by_principal_id {
        return Err(Error::Forbidden);
    }
    let required = required_matrix_operating_facts(context, &revision.input)?;
    if request.evidence.len() != required.len() {
        return Err(Error::InvalidArguments);
    }
    let mut refs = BTreeMap::new();
    for evidence in &request.evidence {
        if evidence.evidence_ref.trim().is_empty()
            || evidence.evidence_ref.len() > 4096
            || refs
                .insert(evidence.fact_path.as_str(), evidence.evidence_ref.as_str())
                .is_some()
        {
            return Err(Error::InvalidArguments);
        }
    }
    let validation_now = clock()?;
    let mut bindings = Vec::with_capacity(required.len());
    for fact in &required {
        let evidence_ref = refs
            .get(fact.path.as_str())
            .ok_or(Error::InvalidArguments)?;
        let binding = validator
            .validate(
                actor.workspace_id,
                revision.task_id,
                revision.revision,
                fact,
                evidence_ref,
                validation_now,
            )
            .await?;
        if binding.fact_path != fact.path
            || binding.value_digest != fact.value_digest
            || binding.evidence_ref != *evidence_ref
            || binding.validation_outcome != EvidenceValidationOutcome::Accepted
        {
            return Err(Error::InvalidArguments);
        }
        bindings.push(binding);
    }
    let mut record = ContextMatrixVerificationRecord {
        schema: CONTEXT_MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: revision.task_id.to_string(),
        task_revision: revision.revision.to_string(),
        frozen_snapshot_id: frozen_snapshot_id.to_string(),
        authority_schema: context.schema().into(),
        input_digest: matrix_input_digest(&revision.input)?,
        requirements_semantic_digest: context.semantic_digest().into(),
        owner_principal: revision.recorded_by_principal_id.to_string(),
        verifier_principal: actor.verifier_principal_id.to_string(),
        policy_version: validator.policy_version().into(),
        bindings,
        digest: String::new(),
    };
    if record.input_digest != revision.input_digest {
        return Err(Error::InternalInvariant);
    }
    record.digest = record.canonical_digest()?;
    evaluate_context_matrix_verification(
        &record.task_id,
        &record.task_revision,
        &record.frozen_snapshot_id,
        &revision.input,
        context,
        &record,
        clock()?,
    )?;
    store
        .append_context_matrix_verification(
            actor.workspace_id,
            actor.verifier_session_id,
            revision.task_id,
            revision.revision,
            &revision.input_digest,
            &record,
        )
        .await?;
    Ok(record)
}

pub(crate) fn current_epoch_seconds() -> Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InternalInvariant)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| Error::InternalInvariant)
}

#[cfg(test)]
mod tests;
