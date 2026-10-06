//! Advisory comparison of two delivery mechanisms. It does not compose policy,
//! rank owner choices, select an approach, or authorize an effect.

use crate::{Error, EvidenceValidationOutcome, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const TECHNICAL_DECISION_SCHEMA: &str = "tect.matrix-technical-decision/1";
pub const DELIVERY_MECHANISM_CARD: &str = "EM02-DELIVERY-MECHANISM@0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryApproachKind {
    ReuseExistingPath,
    SeparateMechanism,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryApproach {
    /// Opaque owner-authored alternative ID; never a technology name rule.
    pub id: String,
    pub kind: DeliveryApproachKind,
    pub title: String,
    pub mechanism: String,
    pub operational_consequences: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechnicalFactKind {
    ReuseSourceSupport,
    SeparateSourceSupport,
    ReuseMeetsOutcome,
    ReuseOperationsAcceptable,
    SeparateMeetsOutcome,
    SeparateOperationsAcceptable,
    SeparateRequiredByConstraint,
}

const REQUIRED_FACTS: [TechnicalFactKind; 7] = [
    TechnicalFactKind::ReuseSourceSupport,
    TechnicalFactKind::SeparateSourceSupport,
    TechnicalFactKind::ReuseMeetsOutcome,
    TechnicalFactKind::ReuseOperationsAcceptable,
    TechnicalFactKind::SeparateMeetsOutcome,
    TechnicalFactKind::SeparateOperationsAcceptable,
    TechnicalFactKind::SeparateRequiredByConstraint,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechnicalSourceSupport {
    Supported,
    Unsupported,
    Forbidden,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum TechnicalFactValue {
    SourceSupport(TechnicalSourceSupport),
    Determination(bool),
}

/// A serialized claim only. `Accepted` is never proof without the trusted
/// validator port resolving this exact assertion against immutable evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalEvidenceBinding {
    pub task_id: String,
    pub task_revision: String,
    pub matrix_verification_digest: String,
    pub evidence_ref: String,
    pub content_digest: String,
    pub validator_policy_version: String,
    pub observed_at: i64,
    pub expires_at: i64,
    pub validation_outcome: EvidenceValidationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TechnicalFactObservation {
    Verified {
        value: TechnicalFactValue,
        binding: TechnicalEvidenceBinding,
    },
    NeedsInspection {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalDecisionFact {
    pub kind: TechnicalFactKind,
    pub observation: TechnicalFactObservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechnicalApprovalAuthority {
    Owner,
    OwnerDelegated,
}

/// A reference to a frozen owner or explicitly delegated approval. Its text
/// is not authority; the trusted port must resolve the record and its scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalOwnerApprovalClaim {
    pub approval_ref: String,
    pub approving_principal: String,
    pub authority: TechnicalApprovalAuthority,
    pub task_id: String,
    pub task_revision: String,
    pub candidate_digest: String,
    pub approved_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryMechanismDecisionCard {
    pub schema: String,
    pub card_id: String,
    pub task_id: String,
    pub task_revision: String,
    pub matrix_verification_digest: String,
    pub decision_question: String,
    pub required_outcome: String,
    pub approaches: Vec<DeliveryApproach>,
    pub owner_approval: TechnicalOwnerApprovalClaim,
    pub facts: Vec<TechnicalDecisionFact>,
}

/// Server-owned implementations resolve current, source-validated evidence
/// and owner/delegated approval. Never implement this from request fields.
pub trait TechnicalDecisionTrust {
    fn validate_fact(
        &self,
        card: &DeliveryMechanismDecisionCard,
        fact: &TechnicalDecisionFact,
        binding: &TechnicalEvidenceBinding,
        now: i64,
    ) -> Result<bool>;

    fn validate_owner_approval(
        &self,
        card: &DeliveryMechanismDecisionCard,
        candidate_digest: &str,
        now: i64,
    ) -> Result<bool>;
}

/// Production remains default-deny until a server-owned trust port exists.
pub struct DenyTechnicalDecisionTrust;

impl TechnicalDecisionTrust for DenyTechnicalDecisionTrust {
    fn validate_fact(
        &self,
        _: &DeliveryMechanismDecisionCard,
        _: &TechnicalDecisionFact,
        _: &TechnicalEvidenceBinding,
        _: i64,
    ) -> Result<bool> {
        Ok(false)
    }

    fn validate_owner_approval(
        &self,
        _: &DeliveryMechanismDecisionCard,
        _: &str,
        _: i64,
    ) -> Result<bool> {
        Ok(false)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechnicalAdequacy {
    Adequate,
    Dominated,
    ViolatesConstraint,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechnicalApproachAssessment {
    pub approach_id: String,
    pub source_support: TechnicalSourceSupport,
    pub engineering_adequacy: TechnicalAdequacy,
    pub eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveryMechanismComparison {
    pub card_id: String,
    pub task_id: String,
    pub task_revision: String,
    pub matrix_verification_digest: String,
    pub card_digest: String,
    pub needs_inspection: Vec<TechnicalFactKind>,
    pub assessments: Vec<TechnicalApproachAssessment>,
    /// Empty for unresolved evidence. This is a set of candidates, not a choice.
    pub eligible_approach_ids: Vec<String>,
}

impl DeliveryMechanismDecisionCard {
    /// Freezes question, outcome, exact candidate IDs and full approaches to
    /// this task/revision, independent of later operating evidence changes.
    pub fn candidate_digest(&self) -> Result<String> {
        let mut approaches = self.approaches.clone();
        approaches.sort_by_key(|approach| approach.kind);
        let bytes = serde_json::to_vec(&(
            &self.schema,
            &self.card_id,
            &self.task_id,
            &self.task_revision,
            &self.matrix_verification_digest,
            &self.decision_question,
            &self.required_outcome,
            &approaches,
        ))
        .map_err(|_| Error::InvalidArguments)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    pub fn canonical_digest(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.approaches.sort_by_key(|approach| approach.kind);
        canonical.facts.sort_by_key(|fact| fact.kind);
        let bytes = serde_json::to_vec(&canonical).map_err(|_| Error::InvalidArguments)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

/// The public no-port route cannot turn deserialized claims into eligibility.
pub fn compare_delivery_mechanisms(
    card: &DeliveryMechanismDecisionCard,
    current_task_id: &str,
    current_task_revision: &str,
    current_matrix_verification_digest: &str,
    now: i64,
) -> Result<DeliveryMechanismComparison> {
    compare_delivery_mechanisms_with_trust(
        card,
        current_task_id,
        current_task_revision,
        current_matrix_verification_digest,
        now,
        &DenyTechnicalDecisionTrust,
    )
}

/// Caller supplies current identity and a server-owned trust port. The port
/// must check exact typed assertions and approval provenance against sources.
pub fn compare_delivery_mechanisms_with_trust(
    card: &DeliveryMechanismDecisionCard,
    current_task_id: &str,
    current_task_revision: &str,
    current_matrix_verification_digest: &str,
    now: i64,
    trust: &impl TechnicalDecisionTrust,
) -> Result<DeliveryMechanismComparison> {
    if card.schema != TECHNICAL_DECISION_SCHEMA
        || card.card_id != DELIVERY_MECHANISM_CARD
        || card.task_id != current_task_id
        || card.task_revision != current_task_revision
        || card.matrix_verification_digest != current_matrix_verification_digest
        || !bounded(&card.task_id, 256)
        || !bounded(&card.task_revision, 256)
        || !sha256(&card.matrix_verification_digest)
        || !bounded(&card.decision_question, 512)
        || !bounded(&card.required_outcome, 512)
        || card.approaches.len() != 2
    {
        return Err(Error::InvalidArguments);
    }
    let approval = &card.owner_approval;
    let candidate_digest = card.candidate_digest()?;
    if approval.task_id != card.task_id
        || approval.task_revision != card.task_revision
        || approval.candidate_digest != candidate_digest
        || !bounded(&approval.approval_ref, 4096)
        || !bounded(&approval.approving_principal, 256)
        || approval.approved_at > now
    {
        return Err(Error::InvalidArguments);
    }
    let mut approaches = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for approach in &card.approaches {
        if !bounded(&approach.id, 128)
            || !bounded(&approach.title, 256)
            || !bounded(&approach.mechanism, 1024)
            || approach.operational_consequences.is_empty()
            || approach.operational_consequences.len() > 16
            || approach
                .operational_consequences
                .iter()
                .any(|item| !bounded(item, 512))
            || !ids.insert(&approach.id)
            || approaches.insert(approach.kind, approach).is_some()
        {
            return Err(Error::InvalidArguments);
        }
    }
    if card.facts.len() != REQUIRED_FACTS.len() {
        return Err(Error::InvalidArguments);
    }
    let mut facts = BTreeMap::new();
    let mut needs_inspection = Vec::new();
    for fact in &card.facts {
        if facts.insert(fact.kind, fact).is_some() {
            return Err(Error::InvalidArguments);
        }
        match &fact.observation {
            TechnicalFactObservation::NeedsInspection { reason } => {
                if !bounded(reason, 512) {
                    return Err(Error::InvalidArguments);
                }
                needs_inspection.push(fact.kind);
            }
            TechnicalFactObservation::Verified { value, binding } => {
                let source = matches!(
                    fact.kind,
                    TechnicalFactKind::ReuseSourceSupport
                        | TechnicalFactKind::SeparateSourceSupport
                );
                if source != matches!(value, TechnicalFactValue::SourceSupport(_))
                    || binding.task_id != card.task_id
                    || binding.task_revision != card.task_revision
                    || binding.matrix_verification_digest != card.matrix_verification_digest
                    || !bounded(&binding.evidence_ref, 4096)
                    || !sha256(&binding.content_digest)
                    || !bounded(&binding.validator_policy_version, 256)
                    || binding.observed_at > now
                    || binding.expires_at <= now
                    || binding.expires_at <= binding.observed_at
                    || binding.validation_outcome != EvidenceValidationOutcome::Accepted
                    || !trust.validate_fact(card, fact, binding, now)?
                {
                    return Err(Error::InvalidArguments);
                }
                if matches!(
                    value,
                    TechnicalFactValue::SourceSupport(TechnicalSourceSupport::Unknown)
                ) {
                    needs_inspection.push(fact.kind);
                }
            }
        }
    }
    if REQUIRED_FACTS.iter().any(|kind| !facts.contains_key(kind)) {
        return Err(Error::InvalidArguments);
    }
    if !trust.validate_owner_approval(card, &candidate_digest, now)? {
        return Err(Error::InvalidArguments);
    }
    needs_inspection.sort();
    let mut comparison = DeliveryMechanismComparison {
        card_id: card.card_id.clone(),
        task_id: card.task_id.clone(),
        task_revision: card.task_revision.clone(),
        matrix_verification_digest: card.matrix_verification_digest.clone(),
        card_digest: card.canonical_digest()?,
        needs_inspection,
        assessments: Vec::new(),
        eligible_approach_ids: Vec::new(),
    };
    if !comparison.needs_inspection.is_empty() {
        for approach in &card.approaches {
            let source_kind = match approach.kind {
                DeliveryApproachKind::ReuseExistingPath => TechnicalFactKind::ReuseSourceSupport,
                DeliveryApproachKind::SeparateMechanism => TechnicalFactKind::SeparateSourceSupport,
            };
            let source_support = match &facts[&source_kind].observation {
                TechnicalFactObservation::Verified {
                    value: TechnicalFactValue::SourceSupport(value),
                    ..
                } => *value,
                _ => TechnicalSourceSupport::Unknown,
            };
            comparison.assessments.push(TechnicalApproachAssessment {
                approach_id: approach.id.clone(),
                source_support,
                engineering_adequacy: TechnicalAdequacy::Unknown,
                eligible: false,
            });
        }
        return Ok(comparison);
    }
    let source = |kind| match &facts[&kind].observation {
        TechnicalFactObservation::Verified {
            value: TechnicalFactValue::SourceSupport(value),
            ..
        } => *value,
        _ => unreachable!("validated source fact"),
    };
    let yes = |kind| match &facts[&kind].observation {
        TechnicalFactObservation::Verified {
            value: TechnicalFactValue::Determination(value),
            ..
        } => *value,
        _ => unreachable!("validated determination fact"),
    };
    let reuse_meets = yes(TechnicalFactKind::ReuseMeetsOutcome);
    let reuse_operations = yes(TechnicalFactKind::ReuseOperationsAcceptable);
    let separate_meets = yes(TechnicalFactKind::SeparateMeetsOutcome);
    let separate_operations = yes(TechnicalFactKind::SeparateOperationsAcceptable);
    let separate_required = yes(TechnicalFactKind::SeparateRequiredByConstraint);
    for approach in &card.approaches {
        let (source_support, engineering_adequacy) = match approach.kind {
            DeliveryApproachKind::ReuseExistingPath => (
                source(TechnicalFactKind::ReuseSourceSupport),
                if separate_required || !reuse_meets || !reuse_operations {
                    TechnicalAdequacy::ViolatesConstraint
                } else {
                    TechnicalAdequacy::Adequate
                },
            ),
            DeliveryApproachKind::SeparateMechanism => (
                source(TechnicalFactKind::SeparateSourceSupport),
                if !separate_meets || !separate_operations {
                    TechnicalAdequacy::ViolatesConstraint
                } else if source(TechnicalFactKind::ReuseSourceSupport)
                    == TechnicalSourceSupport::Supported
                    && reuse_meets
                    && reuse_operations
                    && !separate_required
                {
                    TechnicalAdequacy::Dominated
                } else {
                    TechnicalAdequacy::Adequate
                },
            ),
        };
        let eligible = source_support == TechnicalSourceSupport::Supported
            && engineering_adequacy == TechnicalAdequacy::Adequate;
        if eligible {
            comparison.eligible_approach_ids.push(approach.id.clone());
        }
        comparison.assessments.push(TechnicalApproachAssessment {
            approach_id: approach.id.clone(),
            source_support,
            engineering_adequacy,
            eligible,
        });
    }
    comparison.eligible_approach_ids.sort();
    Ok(comparison)
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max
}

fn sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
#[path = "engineering_matrix_technical_decision_tests.rs"]
mod tests;
