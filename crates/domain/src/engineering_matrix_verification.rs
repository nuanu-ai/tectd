//! Pure, revision-bound verification of Matrix facts. Evidence validation and
//! identity authorization are performed by the application before sealing.

use crate::{
    EngineeringMatrixInput, Error, MatrixEvidenceState, MatrixFact, OperationalFacts,
    OwnerReportedEngineeringMatrixFacts, Result, compose_owner_reported_engineering_matrix,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MATRIX_VERIFICATION_SCHEMA: &str = "tect.matrix-verification/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredMatrixFact {
    pub path: String,
    /// SHA-256 of the canonical serialized fact, including state and provenance.
    pub value_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceValidationOutcome {
    Accepted,
    Rejected,
}

/// The application supplies the outcome only after validating the immutable
/// reference and its content. The domain does not fetch evidence or trust a
/// client-supplied `verified` flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixEvidenceBinding {
    pub fact_path: String,
    pub value_digest: String,
    pub evidence_ref: String,
    pub content_digest: String,
    pub source: String,
    pub subject: String,
    pub observed_at: i64,
    pub expires_at: i64,
    pub validation_outcome: EvidenceValidationOutcome,
}

/// Persist append-only. A mutable task revision or evidence change requires a
/// new record. The verifier principal is authenticated by the application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixVerificationRecord {
    pub schema: String,
    pub task_id: String,
    pub task_revision: String,
    pub input_digest: String,
    pub owner_principal: String,
    pub verifier_principal: String,
    pub policy_version: String,
    pub bindings: Vec<MatrixEvidenceBinding>,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedMatrixVerification {
    pub task_id: String,
    pub task_revision: String,
    pub input_digest: String,
    pub record_digest: String,
}

/// Canonical input digest is independent of operational entry and guarantee order.
pub fn matrix_input_digest(input: &EngineeringMatrixInput) -> Result<String> {
    input.validate()?;
    let mut canonical = input.clone();
    if let OperationalFacts::Reported { entries } = &mut canonical.envelope.operational_facts {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
    }
    if let MatrixFact::Known { value, .. } = &mut canonical.affected_guarantees {
        value.sort();
    }
    digest_json(&(MATRIX_VERIFICATION_SCHEMA, canonical))
}

/// Derives every required path from the complete source projection. Unknown,
/// missing, contradictory, invalid, or inapplicable empty facts fail closed.
pub fn required_matrix_facts(input: &EngineeringMatrixInput) -> Result<Vec<RequiredMatrixFact>> {
    input.validate()?;
    let mut facts = BTreeMap::new();
    add_fact(&mut facts, "/mode", &input.mode, false)?;
    add_fact(&mut facts, "/envelope/scale", &input.envelope.scale, false)?;
    match &input.envelope.operational_facts {
        OperationalFacts::KnownEmpty { .. } => {
            add_serialized(
                &mut facts,
                "/envelope/operational_facts",
                &input.envelope.operational_facts,
            )?;
        }
        OperationalFacts::Reported { entries } => {
            for entry in entries {
                add_fact(
                    &mut facts,
                    &format!("/envelope/operational_facts/{}", escape_path(&entry.name)),
                    &entry.fact,
                    true,
                )?;
            }
        }
        OperationalFacts::Absent => return Err(Error::InvalidArguments),
    }
    add_fact(&mut facts, "/criticality", &input.criticality, false)?;
    add_fact(&mut facts, "/intent", &input.intent, false)?;
    add_fact(&mut facts, "/urgency", &input.urgency, false)?;
    add_fact(
        &mut facts,
        "/promised_behavior",
        &input.promised_behavior,
        false,
    )?;
    add_fact(&mut facts, "/promised_proof", &input.promised_proof, false)?;
    let mut guarantees = input.affected_guarantees.clone();
    if let MatrixFact::Known { value, .. } = &mut guarantees {
        value.sort();
    }
    add_fact(&mut facts, "/affected_guarantees", &guarantees, true)?;
    add_fact(
        &mut facts,
        "/actual_exposure",
        &input.actual_exposure,
        false,
    )?;
    add_fact(
        &mut facts,
        "/demand_commitment",
        &input.demand_commitment,
        false,
    )?;
    add_fact(
        &mut facts,
        "/latency_commitment",
        &input.latency_commitment,
        false,
    )?;
    add_fact(&mut facts, "/urgent_repair", &input.urgent_repair, false)?;
    // Reuse the established cross-field conflict detection in the composer.
    let bound = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        "matrix-verification-probe".into(),
        "1".into(),
        input.clone(),
    )?;
    if compose_owner_reported_engineering_matrix(&bound)
        .unresolved_evidence
        .iter()
        .any(|issue| issue.state != MatrixEvidenceState::KnownEmpty)
    {
        return Err(Error::InvalidArguments);
    }
    Ok(facts
        .into_iter()
        .map(|(path, value_digest)| RequiredMatrixFact { path, value_digest })
        .collect())
}

fn add_fact<T: Serialize>(
    facts: &mut BTreeMap<String, String>,
    path: &str,
    fact: &MatrixFact<T>,
    allow_empty: bool,
) -> Result<()> {
    match fact {
        MatrixFact::Known { .. } => add_serialized(facts, path, fact),
        MatrixFact::KnownEmpty { .. } if allow_empty => add_serialized(facts, path, fact),
        _ => Err(Error::InvalidArguments),
    }
}

fn add_serialized(
    facts: &mut BTreeMap<String, String>,
    path: &str,
    value: &impl Serialize,
) -> Result<()> {
    if facts.insert(path.into(), digest_json(value)?).is_some() {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn escape_path(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

impl MatrixVerificationRecord {
    /// Compute after all fields and bindings have been set; does not validate
    /// an evidence source or authenticate a principal.
    pub fn canonical_digest(&self) -> Result<String> {
        let mut canonical = self.clone();
        canonical.digest.clear();
        canonical
            .bindings
            .sort_by(|a, b| a.fact_path.cmp(&b.fact_path));
        digest_json(&canonical)
    }
}

/// `now` is supplied by the application. A successful result proves exact
/// structural coverage of the recorded input, not external evidence validity.
pub fn evaluate_matrix_verification(
    task_id: &str,
    task_revision: &str,
    input: &EngineeringMatrixInput,
    record: &MatrixVerificationRecord,
    now: i64,
) -> Result<ValidatedMatrixVerification> {
    let required = required_matrix_facts(input)?;
    if record.schema != MATRIX_VERIFICATION_SCHEMA
        || record.task_id != task_id
        || record.task_revision != task_revision
        || !text(&record.task_id, 256)
        || !text(&record.task_revision, 256)
        || !text(&record.owner_principal, 256)
        || !text(&record.verifier_principal, 256)
        || record.owner_principal == record.verifier_principal
        || !text(&record.policy_version, 256)
        || record.input_digest != matrix_input_digest(input)?
        || record.digest != record.canonical_digest()?
        || record.bindings.len() != required.len()
    {
        return Err(Error::InvalidArguments);
    }
    let required: BTreeMap<_, _> = required
        .into_iter()
        .map(|fact| (fact.path, fact.value_digest))
        .collect();
    let mut seen = BTreeSet::new();
    for binding in &record.bindings {
        if !seen.insert(&binding.fact_path)
            || required.get(&binding.fact_path) != Some(&binding.value_digest)
            || !text(&binding.evidence_ref, 4096)
            || !sha256(&binding.content_digest)
            || !text(&binding.source, 256)
            || !text(&binding.subject, 256)
            || binding.observed_at > now
            || binding.expires_at <= now
            || binding.expires_at <= binding.observed_at
            || binding.validation_outcome != EvidenceValidationOutcome::Accepted
        {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(ValidatedMatrixVerification {
        task_id: record.task_id.clone(),
        task_revision: record.task_revision.clone(),
        input_digest: record.input_digest.clone(),
        record_digest: record.digest.clone(),
    })
}

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest_json(value: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidArguments)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
#[path = "engineering_matrix_verification_tests.rs"]
mod tests;
