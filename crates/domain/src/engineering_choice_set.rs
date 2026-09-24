//! Owner-authored engineering alternatives and the bounded Matrix advice contract.
//! Shape and eligibility do not establish feasibility, source authority, or permission to act.

use crate::{
    EngineeringMatrixComposition, EngineeringMatrixInput, Error, MatrixSourceVerificationStatus,
    OperationalFacts, OwnerReportedEngineeringMatrixFacts, Result, VerifiedEngineeringMatrixFacts,
    compose_engineering_matrix, compose_owner_reported_engineering_matrix,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MATRIX_CHOICE_SET_SCHEMA: &str = "tect.matrix-choice-set/1";
pub const MATRIX_EVALUATION_CONTRACT_VERSION: &str = "tect.matrix-ranking/1";
pub const MATRIX_VERIFIED_EVALUATION_CONTRACT_VERSION: &str = "tect.matrix-ranking/2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringCandidate {
    pub candidate_id: String,
    pub title: String,
    pub approach: String,
    /// Matrix fact field IDs, never mandatory-card IDs.
    pub assumption_fact_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringChoiceSet {
    pub schema: String,
    pub choice_set_id: String,
    pub version: u64,
    pub task_id: String,
    pub task_revision: String,
    pub decision_question: String,
    pub candidates: Vec<EngineeringCandidate>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum MatrixAdviceEligibility {
    NotApplicable,
    EligibleForAdvice { candidate_ids: Vec<String> },
}

impl EngineeringChoiceSet {
    /// Accepts zero or one recorded alternative but never sends it for ranking.
    /// The Matrix input supplies the allowed fact IDs, not independent verification.
    pub fn validate(&self, input: &EngineeringMatrixInput) -> Result<MatrixAdviceEligibility> {
        input.validate()?;
        if self.schema != MATRIX_CHOICE_SET_SCHEMA
            || !opaque_id(&self.choice_set_id)
            || !opaque_id(&self.task_id)
            || !opaque_id(&self.task_revision)
            || self.version == 0
            || !bounded_text(&self.decision_question, 1024)
            || self.candidates.len() > 5
        {
            return Err(Error::InvalidArguments);
        }
        let fact_ids = matrix_fact_ids(input);
        let mut candidate_ids = BTreeSet::new();
        for candidate in &self.candidates {
            if !opaque_id(&candidate.candidate_id)
                || !bounded_text(&candidate.title, 256)
                || !bounded_text(&candidate.approach, 4096)
                || !candidate_ids.insert(candidate.candidate_id.clone())
            {
                return Err(Error::InvalidArguments);
            }
            let mut assumptions = BTreeSet::new();
            for fact_id in &candidate.assumption_fact_ids {
                if !fact_ids.contains(fact_id) || !assumptions.insert(fact_id) {
                    return Err(Error::InvalidArguments);
                }
            }
        }
        if candidate_ids.len() < 2 {
            Ok(MatrixAdviceEligibility::NotApplicable)
        } else {
            Ok(MatrixAdviceEligibility::EligibleForAdvice {
                candidate_ids: candidate_ids.into_iter().collect(),
            })
        }
    }

    /// Canonical bytes are independent of candidate and assumption-list order.
    pub fn canonical_digest(&self, input: &EngineeringMatrixInput) -> Result<String> {
        self.validate(input)?;
        let mut canonical = self.clone();
        canonical
            .candidates
            .sort_by(|a, b| a.candidate_id.cmp(&b.candidate_id));
        for candidate in &mut canonical.candidates {
            candidate.assumption_fact_ids.sort();
        }
        sha256_json(&canonical)
    }
}

/// Explicit evaluation binding. A caller must supply the stored input and its
/// composition for the exact revision; this function rejects mismatched or
/// incomplete composition rather than hashing caller-selected obligations.
pub fn matrix_evaluation_digest(
    input: &EngineeringMatrixInput,
    composition: &EngineeringMatrixComposition,
    choice_set: &EngineeringChoiceSet,
) -> Result<Option<String>> {
    let eligibility = choice_set.validate(input)?;
    if composition.task_id != choice_set.task_id
        || composition.task_revision != choice_set.task_revision
    {
        return Err(Error::StaleRevision);
    }
    let composition_status = composition.source_verification_status;
    let expected = match composition_status {
        MatrixSourceVerificationStatus::VerifiedByCaller => compose_engineering_matrix(
            &VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
                choice_set.task_id.clone(),
                choice_set.task_revision.clone(),
                input.clone(),
            )?,
        ),
        MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
        | MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification => {
            let mut composition = compose_owner_reported_engineering_matrix(
                &OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                    choice_set.task_id.clone(),
                    choice_set.task_revision.clone(),
                    input.clone(),
                )?,
            );
            composition.source_verification_status = composition_status;
            composition
        }
    };
    if *composition != expected {
        return Err(Error::InvalidArguments);
    }
    if eligibility == MatrixAdviceEligibility::NotApplicable {
        return Ok(None);
    }
    // Includes the entire stored fact projection (values, provenance and
    // unresolved states), verification status, card IDs/versions and issues.
    sha256_json(&(
        MATRIX_EVALUATION_CONTRACT_VERSION,
        &composition.task_id,
        &composition.task_revision,
        composition.catalogue_version,
        input,
        &composition.source_verification_status,
        &composition.mandatory_cards,
        &composition.unresolved_evidence,
        choice_set.canonical_digest(input)?,
    ))
    .map(Some)
}

/// Positive ranking material is bound to the exact independently validated
/// verification record. The legacy digest above remains unchanged for no-call
/// receipts and replay.
pub fn matrix_verified_evaluation_digest(
    input: &EngineeringMatrixInput,
    composition: &EngineeringMatrixComposition,
    choice_set: &EngineeringChoiceSet,
    verification: &crate::ValidatedMatrixVerification,
) -> Result<String> {
    if composition.source_verification_status
        != MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
        || !verification.matches_input(&choice_set.task_id, &choice_set.task_revision, input)?
    {
        return Err(Error::InvalidArguments);
    }
    let legacy_digest =
        matrix_evaluation_digest(input, composition, choice_set)?.ok_or(Error::InvalidArguments)?;
    sha256_json(&(
        MATRIX_VERIFIED_EVALUATION_CONTRACT_VERSION,
        legacy_digest,
        verification.record_digest(),
    ))
}

/// Exact saved material for an explicit selection, including a single
/// owner-authored choice for which optional ranking is inapplicable.
/// For two or more choices this is identical to the guarded-ranking digest.
pub fn matrix_verified_disposition_digest(
    input: &EngineeringMatrixInput,
    composition: &EngineeringMatrixComposition,
    choice_set: &EngineeringChoiceSet,
    verification: &crate::ValidatedMatrixVerification,
) -> Result<String> {
    if composition.source_verification_status
        != MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
        || !composition.is_resolved()
        || !verification.matches_input(&choice_set.task_id, &choice_set.task_revision, input)?
    {
        return Err(Error::InvalidArguments);
    }
    if matrix_evaluation_digest(input, composition, choice_set)?.is_some() {
        return matrix_verified_evaluation_digest(input, composition, choice_set, verification);
    }
    if choice_set.candidates.len() != 1 {
        return Err(Error::InvalidArguments);
    }
    sha256_json(&(
        "tect.matrix-verified-disposition/1",
        &composition.task_id,
        &composition.task_revision,
        composition.catalogue_version,
        input,
        &composition.source_verification_status,
        &composition.mandatory_cards,
        &composition.unresolved_evidence,
        choice_set.canonical_digest(input)?,
        verification.record_digest(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum MatrixRanking {
    Ranked {
        ranked_candidate_ids: Vec<String>,
        recommended_candidate_id: String,
    },
    Abstained {
        ranked_candidate_ids: Vec<String>,
        recommended_candidate_id: Option<String>,
    },
}

impl MatrixRanking {
    /// Validates the provider result against the exact structurally eligible set.
    pub fn validate(&self, eligibility: &MatrixAdviceEligibility) -> Result<()> {
        let MatrixAdviceEligibility::EligibleForAdvice { candidate_ids } = eligibility else {
            return Err(Error::InvalidArguments);
        };
        match self {
            Self::Ranked {
                ranked_candidate_ids,
                recommended_candidate_id,
            } => {
                if ranked_candidate_ids.len() != candidate_ids.len()
                    || ranked_candidate_ids.first() != Some(recommended_candidate_id)
                    || ranked_candidate_ids.iter().collect::<BTreeSet<_>>().len()
                        != ranked_candidate_ids.len()
                    || ranked_candidate_ids.iter().collect::<BTreeSet<_>>()
                        != candidate_ids.iter().collect::<BTreeSet<_>>()
                {
                    return Err(Error::InvalidArguments);
                }
            }
            Self::Abstained {
                ranked_candidate_ids,
                recommended_candidate_id,
            } if ranked_candidate_ids.is_empty() && recommended_candidate_id.is_none() => {}
            Self::Abstained { .. } => return Err(Error::InvalidArguments),
        }
        Ok(())
    }
}

fn matrix_fact_ids(input: &EngineeringMatrixInput) -> BTreeSet<String> {
    let mut ids = [
        "mode",
        "envelope.scale",
        "envelope.operational_facts",
        "criticality",
        "intent",
        "urgency",
        "promised_behavior",
        "promised_proof",
        "affected_guarantees",
        "actual_exposure",
        "demand_commitment",
        "latency_commitment",
        "urgent_repair",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    if let OperationalFacts::Reported { entries } = &input.envelope.operational_facts {
        for entry in entries {
            ids.insert(format!("envelope.operational_facts.{}", entry.name));
        }
    }
    ids
}

fn opaque_id(value: &str) -> bool {
    bounded_text(value, 256) && !value.chars().any(char::is_whitespace)
}

fn bounded_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn sha256_json(value: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidArguments)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
#[path = "engineering_choice_set_tests.rs"]
mod tests;
