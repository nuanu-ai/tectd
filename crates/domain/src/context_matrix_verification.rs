//! Structural hybrid verification. External evidence validation and explicit
//! owner response authorization remain application responsibilities.
use crate::{
    EffectiveMatrixRequirements, EngineeringMatrixComposition, EngineeringMatrixInput, Error,
    EvidenceValidationOutcome, MatrixEvidenceBinding, Result, bind_matrix_requirements_input,
    compose_declared_requirements_matrix, matrix_input_digest, required_matrix_operating_facts,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

pub const CONTEXT_MATRIX_VERIFICATION_SCHEMA: &str = "tect.context-matrix-verification/1";
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextMatrixVerificationRecord {
    pub schema: String,
    pub task_id: String,
    pub task_revision: String,
    pub frozen_snapshot_id: String,
    pub authority_schema: String,
    pub input_digest: String,
    pub requirements_semantic_digest: String,
    pub owner_principal: String,
    pub verifier_principal: String,
    pub policy_version: String,
    pub bindings: Vec<MatrixEvidenceBinding>,
    pub digest: String,
}
impl ContextMatrixVerificationRecord {
    pub fn canonical_digest(&self) -> Result<String> {
        let mut copy = self.clone();
        copy.digest.clear();
        copy.bindings.sort_by(|a, b| a.fact_path.cmp(&b.fact_path));
        let bytes = serde_json::to_vec(&copy).map_err(|_| Error::InvalidArguments)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedContextMatrixVerification {
    task_id: String,
    task_revision: String,
    frozen_snapshot_id: String,
    authority_schema: String,
    input_digest: String,
    requirements_semantic_digest: String,
    record_digest: String,
    evaluated_at: i64,
    expires_at: Option<i64>,
}
impl ValidatedContextMatrixVerification {
    pub fn record_digest(&self) -> &str {
        &self.record_digest
    }
    fn matches(
        &self,
        task_id: &str,
        task_revision: &str,
        expected_frozen_snapshot_id: &str,
        input: &EngineeringMatrixInput,
        context: &EffectiveMatrixRequirements,
        now: i64,
    ) -> Result<bool> {
        let bound = bind_matrix_requirements_input(context, input)?;
        Ok(self.task_id == task_id
            && self.task_revision == task_revision
            && self.frozen_snapshot_id == expected_frozen_snapshot_id
            && self.authority_schema == context.schema()
            && self.input_digest == matrix_input_digest(&bound)?
            && self.requirements_semantic_digest == context.semantic_digest()
            && now >= self.evaluated_at
            && self.expires_at.is_none_or(|expires| now < expires))
    }
}

/// Currentness is classified only after every non-expiry integrity check passes.
/// Expired evidence never carries a validated authority token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextMatrixVerificationCurrentness {
    Current(ValidatedContextMatrixVerification),
    Expired,
}

/// Exact coverage of the required operating subset, never of declared promises.
/// Accepted outcomes are supplied only after the application checks the reference.
pub fn evaluate_context_matrix_verification(
    task_id: &str,
    task_revision: &str,
    expected_frozen_snapshot_id: &str,
    input: &EngineeringMatrixInput,
    context: &EffectiveMatrixRequirements,
    record: &ContextMatrixVerificationRecord,
    now: i64,
) -> Result<ValidatedContextMatrixVerification> {
    match classify_context_matrix_verification(
        task_id,
        task_revision,
        expected_frozen_snapshot_id,
        input,
        context,
        record,
        now,
    )? {
        ContextMatrixVerificationCurrentness::Current(validated) => Ok(validated),
        ContextMatrixVerificationCurrentness::Expired => Err(Error::InvalidArguments),
    }
}

/// Validate the whole record before distinguishing valid-but-expired evidence.
/// All structural failures retain their original errors.
pub fn classify_context_matrix_verification(
    task_id: &str,
    task_revision: &str,
    expected_frozen_snapshot_id: &str,
    input: &EngineeringMatrixInput,
    context: &EffectiveMatrixRequirements,
    record: &ContextMatrixVerificationRecord,
    now: i64,
) -> Result<ContextMatrixVerificationCurrentness> {
    let bound = bind_matrix_requirements_input(context, input)?;
    let required = required_matrix_operating_facts(context, input)?;
    if record.schema != CONTEXT_MATRIX_VERIFICATION_SCHEMA
        || record.task_id != task_id
        || record.task_revision != task_revision
        || !canonical_snapshot_id(expected_frozen_snapshot_id)
        || record.frozen_snapshot_id != expected_frozen_snapshot_id
        || record.authority_schema != context.schema()
        || !text(&record.task_id, 256)
        || !text(&record.task_revision, 256)
        || !text(&record.owner_principal, 256)
        || !text(&record.verifier_principal, 256)
        || record.owner_principal == record.verifier_principal
        || !text(&record.policy_version, 256)
        || record.input_digest != matrix_input_digest(&bound)?
        || record.requirements_semantic_digest != context.semantic_digest()
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
            || binding.expires_at <= binding.observed_at
            || binding.validation_outcome != EvidenceValidationOutcome::Accepted
        {
            return Err(Error::InvalidArguments);
        }
    }
    if record
        .bindings
        .iter()
        .any(|binding| binding.expires_at <= now)
    {
        return Ok(ContextMatrixVerificationCurrentness::Expired);
    }
    Ok(ContextMatrixVerificationCurrentness::Current(
        ValidatedContextMatrixVerification {
            task_id: record.task_id.clone(),
            task_revision: record.task_revision.clone(),
            frozen_snapshot_id: record.frozen_snapshot_id.clone(),
            authority_schema: record.authority_schema.clone(),
            input_digest: record.input_digest.clone(),
            requirements_semantic_digest: record.requirements_semantic_digest.clone(),
            record_digest: record.digest.clone(),
            evaluated_at: now,
            expires_at: record
                .bindings
                .iter()
                .map(|binding| binding.expires_at)
                .min(),
        },
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextMatrixResolutionStatus {
    ConfirmedRequirementsValidatedOperatingEvidence,
}
/// Separate resolution surface: never relabels a legacy owner-report record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextEngineeringMatrixComposition {
    composition: EngineeringMatrixComposition,
    status: ContextMatrixResolutionStatus,
    frozen_snapshot_id: String,
    authority_schema: String,
    requirements_semantic_digest: String,
    operating_verification_digest: String,
}
impl ContextEngineeringMatrixComposition {
    pub fn composition(&self) -> &EngineeringMatrixComposition {
        &self.composition
    }
    pub fn status(&self) -> ContextMatrixResolutionStatus {
        self.status
    }
    pub fn frozen_snapshot_id(&self) -> &str {
        &self.frozen_snapshot_id
    }
    pub fn authority_schema(&self) -> &str {
        &self.authority_schema
    }
    pub fn requirements_semantic_digest(&self) -> &str {
        &self.requirements_semantic_digest
    }
    pub fn operating_verification_digest(&self) -> &str {
        &self.operating_verification_digest
    }
    pub fn is_resolved(&self) -> bool {
        self.composition.unresolved_evidence.is_empty()
    }
}
pub fn compose_confirmed_requirements_matrix(
    task_id: &str,
    task_revision: &str,
    expected_frozen_snapshot_id: &str,
    input: &EngineeringMatrixInput,
    context: &EffectiveMatrixRequirements,
    validated: &ValidatedContextMatrixVerification,
    now: i64,
) -> Result<ContextEngineeringMatrixComposition> {
    if !canonical_snapshot_id(expected_frozen_snapshot_id)
        || !validated.matches(
            task_id,
            task_revision,
            expected_frozen_snapshot_id,
            input,
            context,
            now,
        )?
    {
        return Err(Error::StaleRevision);
    }
    let composition =
        compose_declared_requirements_matrix(context, task_id.into(), task_revision.into(), input)?;
    if !composition.unresolved_evidence.is_empty() {
        return Err(Error::InvalidArguments);
    }
    Ok(ContextEngineeringMatrixComposition {
        composition,
        status: ContextMatrixResolutionStatus::ConfirmedRequirementsValidatedOperatingEvidence,
        frozen_snapshot_id: validated.frozen_snapshot_id.clone(),
        authority_schema: context.schema().into(),
        requirements_semantic_digest: context.semantic_digest().into(),
        operating_verification_digest: validated.record_digest.clone(),
    })
}
fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn canonical_snapshot_id(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
}

#[cfg(test)]
#[path = "context_matrix_verification_tests.rs"]
mod tests;
