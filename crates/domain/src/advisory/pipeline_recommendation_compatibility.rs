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
pub const PIPELINE_RECOMMENDATION_CATALOGUE_REVISION: &str = "4";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCompatibilityPolicy {
    pub version: String,
    /// This snapshot applies to one saved Matrix task and catalogue revision.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub task_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub task_revision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub catalogue_revision: String,
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

pub(crate) struct PipelineCompatibilityContext<'a> {
    pub task_id: &'a str,
    pub task_revision: &'a str,
    pub catalogue_revision: &'a str,
    pub input: &'a EngineeringMatrixInput,
    pub input_digest: &'a str,
    pub selected_candidate_id: &'a str,
    pub mandatory_cards: &'a BTreeSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineExclusionReason {
    UnsupportedPolicy,
    StaleTask,
    StaleCatalogue,
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
            task_id: String::new(),
            task_revision: String::new(),
            catalogue_revision: String::new(),
            rules: Vec::new(),
        }
    }

    /// Reject malformed host configuration before it can become an available
    /// policy. Task-specific card and obligation matches are checked later
    /// against the saved Matrix and current definitions.
    pub fn validate_host_snapshot(&self, current_catalogue_revision: &str) -> Result<()> {
        if self.version != PIPELINE_COMPATIBILITY_POLICY_VERSION
            || self.task_id.trim().is_empty()
            || self.task_revision.trim().is_empty()
            || self.catalogue_revision != current_catalogue_revision
            || self.rules.is_empty()
        {
            return Err(Error::InvalidConfiguration);
        }
        let mut kinds = BTreeSet::new();
        for rule in &self.rules {
            if !PipelineKind::CURRENT_SLICE_RUN_KINDS.contains(&rule.kind)
                || !kinds.insert(rule.kind)
                || !valid_digest(&rule.matrix_input_digest)
                || rule.allowed_modes.is_empty()
                || rule
                    .allowed_modes
                    .iter()
                    .enumerate()
                    .any(|(index, mode)| rule.allowed_modes[index + 1..].contains(mode))
                || rule.selected_candidate_ids.is_empty()
                || rule
                    .selected_candidate_ids
                    .iter()
                    .any(|id| id.trim().is_empty())
                || rule
                    .selected_candidate_ids
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != rule.selected_candidate_ids.len()
                || rule.card_coverage.is_empty()
                || rule.card_coverage.iter().any(|coverage| {
                    coverage.card_id.trim().is_empty()
                        || coverage.phase_id.trim().is_empty()
                        || !valid_digest(&coverage.obligation_digest)
                })
                || rule
                    .card_coverage
                    .iter()
                    .map(|coverage| &coverage.card_id)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != rule.card_coverage.len()
            {
                return Err(Error::InvalidConfiguration);
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        // Preserve rule and coverage order: any policy edit changes the digest.
        digest_json(self)
    }

    pub(crate) fn reason_for(
        &self,
        kind: PipelineKind,
        context: &PipelineCompatibilityContext<'_>,
        option: &PipelineRecommendationOption,
    ) -> Option<PipelineExclusionReason> {
        if self.version != PIPELINE_COMPATIBILITY_POLICY_VERSION {
            return Some(PipelineExclusionReason::UnsupportedPolicy);
        }
        if self.task_id != context.task_id || self.task_revision != context.task_revision {
            return Some(PipelineExclusionReason::StaleTask);
        }
        if self.catalogue_revision != context.catalogue_revision {
            return Some(PipelineExclusionReason::StaleCatalogue);
        }
        let mut matching = self.rules.iter().filter(|rule| rule.kind == kind);
        let Some(rule) = matching.next() else {
            return Some(PipelineExclusionReason::MissingRule);
        };
        if matching.next().is_some() {
            return Some(PipelineExclusionReason::AmbiguousRule);
        }
        if rule.matrix_input_digest != context.input_digest {
            return Some(PipelineExclusionReason::StaleMatrixInput);
        }
        let MatrixFact::Known { value: mode, .. } = &context.input.mode else {
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
                .filter(|id| id.as_str() == context.selected_candidate_id)
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
        if covered != *context.mandatory_cards || covered.len() != rule.card_coverage.len() {
            return Some(PipelineExclusionReason::IncompleteCardCoverage);
        }
        for coverage in &rule.card_coverage {
            let Some(obligation) = option
                .verification_plan
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

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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
