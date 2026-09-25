//! Explicit, versioned Matrix-to-pipeline compatibility. The caller supplies
//! policy; catalogue prose and provider output never mint eligibility.

use crate::{
    EngineeringMatrixInput, EngineeringMode, Error, MatrixFact, PipelineKind,
    PipelineRecommendationOption, PipelineVerificationObligation, Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const PIPELINE_COMPATIBILITY_POLICY_VERSION: &str = "tect.pipeline-matrix-compatibility/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCompatibilityPolicy {
    pub version: String,
    /// A missing kind is excluded. No default or prose-based compatibility.
    pub rules: Vec<PipelineCompatibilityRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCompatibilityRule {
    pub kind: PipelineKind,
    /// Pins the exact verified fact projection, including provenance.
    pub matrix_input_digest: String,
    pub allowed_modes: Vec<EngineeringMode>,
    pub selected_candidate_ids: Vec<String>,
    /// Each Matrix-mandated card must map to one required phase obligation.
    pub card_coverage: Vec<PipelineCardCoverage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCardCoverage {
    pub card_id: String,
    pub phase_id: String,
    /// Digest of the full required phase verification obligation.
    pub obligation_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineExclusionReason {
    UnsupportedPolicy,
    MissingRule,
    AmbiguousRule,
    StaleMatrixInput,
    IncompatibleMode,
    IncompatibleCandidate,
    IncompleteCardCoverage,
    MissingPhaseObligation,
    StalePhaseObligation,
    UnavailableDefinition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineExcludedKind {
    pub kind: PipelineKind,
    pub reason: PipelineExclusionReason,
}

impl PipelineCompatibilityPolicy {
    /// A missing host configuration is represented as an explicit deny-all
    /// snapshot so the captured material remains digestible and inspectable.
    pub fn unavailable() -> Self {
        Self {
            version: "unavailable".into(),
            rules: Vec::new(),
        }
    }

    pub fn digest(&self) -> Result<String> {
        // Preserve rule and coverage order: any policy edit changes the digest.
        digest_json(self)
    }

    pub(crate) fn reason_for(
        &self,
        kind: PipelineKind,
        input: &EngineeringMatrixInput,
        input_digest: &str,
        selected_candidate_id: &str,
        mandatory_cards: &BTreeSet<String>,
        option: &PipelineRecommendationOption,
    ) -> Option<PipelineExclusionReason> {
        if self.version != PIPELINE_COMPATIBILITY_POLICY_VERSION {
            return Some(PipelineExclusionReason::UnsupportedPolicy);
        }
        let mut matching = self.rules.iter().filter(|rule| rule.kind == kind);
        let Some(rule) = matching.next() else {
            return Some(PipelineExclusionReason::MissingRule);
        };
        if matching.next().is_some() {
            return Some(PipelineExclusionReason::AmbiguousRule);
        }
        if rule.matrix_input_digest != input_digest {
            return Some(PipelineExclusionReason::StaleMatrixInput);
        }
        let MatrixFact::Known { value: mode, .. } = &input.mode else {
            return Some(PipelineExclusionReason::IncompatibleMode);
        };
        if rule.allowed_modes.is_empty()
            || rule
                .allowed_modes
                .iter()
                .filter(|candidate| *candidate == mode)
                .count()
                != 1
        {
            return Some(PipelineExclusionReason::IncompatibleMode);
        }
        if rule.selected_candidate_ids.is_empty()
            || rule
                .selected_candidate_ids
                .iter()
                .filter(|id| id.as_str() == selected_candidate_id)
                .count()
                != 1
        {
            return Some(PipelineExclusionReason::IncompatibleCandidate);
        }
        let covered = rule
            .card_coverage
            .iter()
            .map(|coverage| coverage.card_id.clone())
            .collect::<BTreeSet<_>>();
        if covered != *mandatory_cards || covered.len() != rule.card_coverage.len() {
            return Some(PipelineExclusionReason::IncompleteCardCoverage);
        }
        for coverage in &rule.card_coverage {
            let Some(obligation) = option
                .obligations
                .iter()
                .find(|obligation| obligation.phase_id == coverage.phase_id)
            else {
                return Some(PipelineExclusionReason::MissingPhaseObligation);
            };
            if !has_verification_duty(obligation)
                || digest_json(obligation).ok().as_deref()
                    != Some(coverage.obligation_digest.as_str())
            {
                return Some(PipelineExclusionReason::StalePhaseObligation);
            }
        }
        None
    }
}

pub fn pipeline_obligation_digest(obligation: &PipelineVerificationObligation) -> Result<String> {
    digest_json(obligation)
}

fn has_verification_duty(obligation: &PipelineVerificationObligation) -> bool {
    !obligation.required_fields.is_empty()
        || !obligation.required_artifacts.is_empty()
        || !obligation.validator_contracts.is_empty()
        || !obligation.output_constraints.is_empty()
        || obligation.fresh_reviewer_input
}

fn digest_json<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidArguments)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
