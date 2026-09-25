use crate::{
    MatrixEvidenceValidator, MatrixTaskRevision, MatrixVerificationStore, TransactionMode,
    WorkspaceService,
};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};
use tect_domain::{
    Error, EvidenceValidationOutcome, MATRIX_VERIFICATION_SCHEMA, MatrixVerificationRecord,
    PrincipalRole, RequestContext, Result, evaluate_matrix_verification, matrix_input_digest,
    required_matrix_facts,
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
    ) -> Result<MatrixVerificationRecord> {
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
        let revision = tx
            .lock_matrix_task(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        // The store must be configured before external evidence is consulted.
        let store = tx
            .matrix_verification_store()
            .ok_or(Error::StorageUnavailable)?;
        let record = verify_locked_revision(
            store,
            self.matrix_evidence_validator.as_ref(),
            workspace.id,
            identity.principal_id,
            session.id,
            &revision,
            request,
            &current_epoch_seconds,
        )
        .await?;
        tx.commit().await?;
        Ok(record)
    }
}

pub(crate) async fn verify_locked_revision(
    store: &mut dyn MatrixVerificationStore,
    validator: &dyn MatrixEvidenceValidator,
    workspace_id: Uuid,
    verifier_principal_id: Uuid,
    verifier_session_id: Uuid,
    revision: &MatrixTaskRevision,
    request: &VerifyMatrixTask,
    clock: &(dyn Fn() -> Result<i64> + Sync),
) -> Result<MatrixVerificationRecord> {
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

pub(crate) fn current_epoch_seconds() -> Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InternalInvariant)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| Error::InternalInvariant)
}

#[cfg(test)]
mod tests;
