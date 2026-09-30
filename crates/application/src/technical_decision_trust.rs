//! Freeze asynchronous, server-resolved records for the synchronous domain
//! comparison. A Matrix `Accepted` binding alone cannot establish the meaning
//! of a delivery-mechanism determination.

use async_trait::async_trait;
use std::collections::BTreeMap;
use tect_domain::{
    DeliveryMechanismDecisionCard, Error, EvidenceValidationOutcome, Result, TechnicalDecisionFact,
    TechnicalDecisionTrust, TechnicalEvidenceBinding, TechnicalFactKind, TechnicalFactObservation,
    TechnicalOwnerApprovalClaim,
};

const FACT_KINDS: [TechnicalFactKind; 7] = [
    TechnicalFactKind::ReuseSourceSupport,
    TechnicalFactKind::SeparateSourceSupport,
    TechnicalFactKind::ReuseMeetsOutcome,
    TechnicalFactKind::ReuseOperationsAcceptable,
    TechnicalFactKind::SeparateMeetsOutcome,
    TechnicalFactKind::SeparateOperationsAcceptable,
    TechnicalFactKind::SeparateRequiredByConstraint,
];

/// Implement only from an independently authenticated, current source. The
/// existing Matrix evidence validator checks operating fact hashes but cannot
/// resolve these seven typed conclusions. The approval resolver must check an
/// exact persisted choice-set digest, the recorded principal and authority,
/// and an exact mapping to the technical card's candidate digest. Request
/// fields, free text, and an `Accepted` flag do not satisfy this contract.
#[async_trait]
pub(crate) trait TechnicalDecisionSnapshotSource: Send + Sync {
    async fn resolve_fact(
        &self,
        card: &DeliveryMechanismDecisionCard,
        kind: TechnicalFactKind,
        now: i64,
    ) -> Result<Option<TechnicalDecisionFact>>;

    async fn resolve_owner_approval(
        &self,
        card: &DeliveryMechanismDecisionCard,
        candidate_digest: &str,
        now: i64,
    ) -> Result<Option<TechnicalOwnerApprovalClaim>>;
}

/// The snapshot is intentionally short lived and tied to one exact card and
/// verification digest. Callers must construct and consume it under the same
/// task-head/currentness lock; it never authorizes a later automatic effect.
pub(crate) struct TechnicalDecisionTrustSnapshot {
    card_digest: String,
    facts: BTreeMap<TechnicalFactKind, TechnicalDecisionFact>,
    approval: TechnicalOwnerApprovalClaim,
    now: i64,
}

pub(crate) async fn build_technical_decision_trust_snapshot(
    source: &dyn TechnicalDecisionSnapshotSource,
    card: &DeliveryMechanismDecisionCard,
    now: i64,
) -> Result<TechnicalDecisionTrustSnapshot> {
    let candidate_digest = card.candidate_digest()?;
    let card_digest = card.canonical_digest()?;
    let mut facts = BTreeMap::new();
    for kind in FACT_KINDS {
        let fact = source
            .resolve_fact(card, kind, now)
            .await?
            .ok_or(Error::Forbidden)?;
        let TechnicalFactObservation::Verified { binding, .. } = &fact.observation else {
            return Err(Error::Forbidden);
        };
        if fact.kind != kind
            || binding.task_id != card.task_id
            || binding.task_revision != card.task_revision
            || binding.matrix_verification_digest != card.matrix_verification_digest
            || binding.observed_at > now
            || binding.expires_at <= now
            || binding.validation_outcome != EvidenceValidationOutcome::Accepted
        {
            return Err(Error::Forbidden);
        }
        facts.insert(kind, fact);
    }
    let approval = source
        .resolve_owner_approval(card, &candidate_digest, now)
        .await?
        .ok_or(Error::Forbidden)?;
    if approval.task_id != card.task_id
        || approval.task_revision != card.task_revision
        || approval.candidate_digest != candidate_digest
        || approval.approved_at > now
    {
        return Err(Error::Forbidden);
    }
    Ok(TechnicalDecisionTrustSnapshot {
        card_digest,
        facts,
        approval,
        now,
    })
}

impl TechnicalDecisionTrust for TechnicalDecisionTrustSnapshot {
    fn validate_fact(
        &self,
        card: &DeliveryMechanismDecisionCard,
        fact: &TechnicalDecisionFact,
        binding: &TechnicalEvidenceBinding,
        now: i64,
    ) -> Result<bool> {
        Ok(now == self.now
            && card.canonical_digest()? == self.card_digest
            && self.facts.get(&fact.kind) == Some(fact)
            && matches!(&fact.observation, TechnicalFactObservation::Verified { binding: saved, .. } if saved == binding))
    }

    fn validate_owner_approval(
        &self,
        card: &DeliveryMechanismDecisionCard,
        candidate_digest: &str,
        now: i64,
    ) -> Result<bool> {
        Ok(now == self.now
            && card.canonical_digest()? == self.card_digest
            && candidate_digest == self.approval.candidate_digest
            && card.owner_approval == self.approval)
    }
}

#[cfg(test)]
#[path = "technical_decision_trust_tests.rs"]
pub(crate) mod tests;
