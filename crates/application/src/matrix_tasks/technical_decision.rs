use super::*;
use crate::{
    ResolvedTechnicalDecisionEvidence, ServerTechnicalDecisionTaskBinding,
    TechnicalDecisionEvidenceReference,
};
use tect_domain::{DeliveryMechanismComparison, TechnicalFactObservation};

/// Caller supplies identity pins only, never conclusions or authority claims.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareTechnicalDeliveryMechanisms {
    pub task_id: Uuid,
    pub expected_task_revision: i64,
    pub operating_verification_digest: String,
    pub evidence_reference: TechnicalDecisionEvidenceReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TechnicalDeliveryMechanismRead {
    Unavailable,
    Compared(DeliveryMechanismComparison),
}

impl WorkspaceService {
    /// Read-only comparison with ancestry and task-head locks held until the
    /// comparison finishes. ReadWrite permits row locks, but no record, advice,
    /// disposition, planning effect, model call or catalogue change occurs.
    pub async fn compare_technical_delivery_mechanisms(
        &self,
        context: &RequestContext,
        request: &CompareTechnicalDeliveryMechanisms,
    ) -> Result<TechnicalDeliveryMechanismRead> {
        validate_request(request)?;
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let source = tx
            .matrix_task_source(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        let requirements = source
            .requirements_binding
            .as_ref()
            .ok_or(Error::Forbidden)?;
        let effective = crate::matrix_verification::lock_and_load_bound_matrix_context(
            tx.matrix_requirements_context_store()
                .ok_or(Error::StorageUnavailable)?,
            workspace.id,
            identity.principal_id,
            requirements,
        )
        .await
        .map_err(|_| Error::StaleRevision)?;
        let revision = lock_current_source(&mut *tx, workspace.id, &source, request).await?;
        let now = crate::matrix_verification::current_epoch_seconds()?;
        let Some((composition, verification)) = binding::compose_bound_revision_with_verification(
            tx.context_matrix_verification_store(),
            self.matrix_evidence_validator.as_ref(),
            workspace.id,
            &revision,
            requirements.snapshot_id,
            &effective,
            now,
        )
        .await?
        else {
            tx.commit().await?;
            return Ok(TechnicalDeliveryMechanismRead::Unavailable);
        };
        if verification.digest != request.operating_verification_digest
            || composition.operating_verification_digest() != request.operating_verification_digest
        {
            return Err(Error::StaleRevision);
        }
        if !composition.is_resolved() {
            tx.commit().await?;
            return Ok(TechnicalDeliveryMechanismRead::Unavailable);
        }
        let Some(choice_set) = revision.choice_set.as_ref() else {
            tx.commit().await?;
            return Ok(TechnicalDeliveryMechanismRead::Unavailable);
        };
        let digest = choice_set.canonical_digest(&revision.input)?;
        if revision.choice_set_digest.as_deref() != Some(digest.as_str()) {
            return Err(Error::Forbidden);
        }
        let server_binding = ServerTechnicalDecisionTaskBinding {
            tenant_id: identity.tenant_id,
            workspace_id: workspace.id,
            task_id: revision.task_id,
            task_revision: revision.revision,
            operating_verification_digest: verification.digest,
            operating_policy_version: verification.policy_version,
            requirements_binding: requirements.clone(),
            choice_set: choice_set.clone(),
            choice_set_digest: digest,
            recorded_by_principal_id: revision.recorded_by_principal_id,
        };
        let resolved = self
            .technical_decision_evidence_resolver
            .resolve(&server_binding, &request.evidence_reference, now)
            .await?;
        let result = match resolved {
            Some(resolved) => {
                compare_resolved(&server_binding, &request.evidence_reference, &resolved, now).await
            }
            None => TechnicalDeliveryMechanismRead::Unavailable,
        };
        tx.commit().await?;
        Ok(result)
    }
}

/// Actual task gate shared by the service and narrow offline store tests.
/// Requirements locks are already held before entering this function.
async fn lock_current_source<T: crate::MatrixTaskStore + ?Sized>(
    store: &mut T,
    workspace_id: Uuid,
    source: &MatrixTaskSource,
    request: &CompareTechnicalDeliveryMechanisms,
) -> Result<MatrixTaskRevision> {
    let revision = store
        .lock_matrix_task(workspace_id, request.task_id)
        .await?
        .ok_or(Error::NotFound)?;
    let current = store
        .matrix_task_source(workspace_id, request.task_id)
        .await?
        .ok_or(Error::NotFound)?;
    if source != &current
        || source.revision != revision
        || revision.task_id != request.task_id
        || revision.revision != request.expected_task_revision
    {
        return Err(Error::StaleRevision);
    }
    Ok(revision)
}

fn validate_request(request: &CompareTechnicalDeliveryMechanisms) -> Result<()> {
    let reference = &request.evidence_reference;
    if request.task_id.is_nil()
        || request.expected_task_revision < 1
        || !sha256(&request.operating_verification_digest)
        || reference.artifact_id.is_nil()
        || reference.artifact_version < 1
        || !sha256(&reference.content_sha256)
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

fn validate_resolved(
    binding: &ServerTechnicalDecisionTaskBinding,
    reference: &TechnicalDecisionEvidenceReference,
    resolved: &ResolvedTechnicalDecisionEvidence,
    now: i64,
) -> Result<()> {
    let card = &resolved.card;
    let approval = &resolved.approval;
    if &resolved.binding != binding
        || &resolved.reference != reference
        || card.task_id != binding.task_id.to_string()
        || card.task_revision != binding.task_revision.to_string()
        || card.matrix_verification_digest != binding.operating_verification_digest
        || card.decision_question != binding.choice_set.decision_question
        || approval.card_digest != card.canonical_digest()?
        || approval.candidate_digest != card.candidate_digest()?
        || approval.claim != card.owner_approval
        || approval.choice_set_digest != binding.choice_set_digest
        || approval.recorded_by_principal_id != binding.recorded_by_principal_id
        || approval.owner_author_principal_id != binding.recorded_by_principal_id
        || approval.owner_author_principal_id.is_nil()
        || approval.owner_authorship_ref.trim().is_empty()
        || approval.owner_authorship_ref.len() > 4096
        || resolved.validator_policy_version != crate::TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION
        || resolved.max_age_seconds <= 0
        || binding.choice_set.task_id != card.task_id
        || binding.choice_set.task_revision != card.task_revision
        || binding.choice_set.candidates.len() != 2
        || card.approaches.len() != 2
        || resolved.candidate_mapping.len() != 2
    {
        return Err(Error::Forbidden);
    }
    let mut ids = std::collections::BTreeSet::new();
    for mapping in &resolved.candidate_mapping {
        let saved = &mapping.frozen_candidate;
        if !ids.insert(&saved.candidate_id)
            || !binding.choice_set.candidates.contains(saved)
            || !card.approaches.contains(&mapping.technical_approach)
            || saved.candidate_id != mapping.technical_approach.id
            || saved.title != mapping.technical_approach.title
            || saved.approach != mapping.technical_approach.mechanism
        {
            return Err(Error::Forbidden);
        }
    }
    if resolved.facts.len() != 7 || card.facts.len() != 7 {
        return Err(Error::Forbidden);
    }
    let mut kinds = std::collections::BTreeSet::new();
    for fact in &resolved.facts {
        let TechnicalFactObservation::Verified {
            binding: evidence, ..
        } = &fact.observation
        else {
            return Err(Error::Forbidden);
        };
        if !kinds.insert(fact.kind)
            || !card.facts.contains(fact)
            || evidence.validator_policy_version != resolved.validator_policy_version
            || evidence.observed_at > now
            || now
                .checked_sub(evidence.observed_at)
                .is_none_or(|age| age > resolved.max_age_seconds)
        {
            return Err(Error::Forbidden);
        }
    }
    Ok(())
}

/// Freeze this resolver's single immutable read for the existing private gate.
#[async_trait::async_trait]
impl crate::technical_decision_trust::TechnicalDecisionSnapshotSource
    for ResolvedTechnicalDecisionEvidence
{
    async fn resolve_fact(
        &self,
        _: &tect_domain::DeliveryMechanismDecisionCard,
        kind: tect_domain::TechnicalFactKind,
        _: i64,
    ) -> Result<Option<tect_domain::TechnicalDecisionFact>> {
        Ok(self.facts.iter().find(|fact| fact.kind == kind).cloned())
    }
    async fn resolve_owner_approval(
        &self,
        _: &tect_domain::DeliveryMechanismDecisionCard,
        _: &str,
        _: i64,
    ) -> Result<Option<tect_domain::TechnicalOwnerApprovalClaim>> {
        Ok(Some(self.approval.claim.clone()))
    }
}

async fn compare_resolved(
    binding: &ServerTechnicalDecisionTaskBinding,
    reference: &TechnicalDecisionEvidenceReference,
    resolved: &ResolvedTechnicalDecisionEvidence,
    now: i64,
) -> TechnicalDeliveryMechanismRead {
    if validate_resolved(binding, reference, resolved, now).is_err() {
        return TechnicalDeliveryMechanismRead::Unavailable;
    }
    let Ok(snapshot) = crate::technical_decision_trust::build_technical_decision_trust_snapshot(
        resolved,
        &resolved.card,
        now,
    )
    .await
    else {
        return TechnicalDeliveryMechanismRead::Unavailable;
    };
    match tect_domain::compare_delivery_mechanisms_with_trust(
        &resolved.card,
        &binding.task_id.to_string(),
        &binding.task_revision.to_string(),
        &binding.operating_verification_digest,
        now,
        &snapshot,
    ) {
        Ok(comparison) => TechnicalDeliveryMechanismRead::Compared(comparison),
        Err(_) => TechnicalDeliveryMechanismRead::Unavailable,
    }
}

#[cfg(test)]
#[path = "technical_decision_tests.rs"]
mod tests;
