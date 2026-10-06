//! Closed, read-only recommendation material for one saved Work node before `slice.open`.
//! The caller loads current records; this module cannot open a Slice or verify a phase.

use crate::{
    ContextEngineeringMatrixComposition, EngineeringChoiceSet, EngineeringMatrixComposition,
    EngineeringMatrixInput, Error, MatrixSourceVerificationStatus,
    OwnerReportedEngineeringMatrixFacts, PIPELINE_RECOMMENDATION_CATALOGUE_REVISION,
    PipelineCatalogueSnapshot, PipelineCompatibilityContext, PipelineCompatibilityPolicy,
    PipelineDefinitionSnapshot, PipelineExcludedKind, PipelineExclusionReason,
    PipelineExecutionOwner, PipelineKind, PipelineVerificationPlan, Result, SliceCandidateNode,
    VerifiedEngineeringMatrixFacts, compose_engineering_matrix,
    compose_owner_reported_engineering_matrix, matrix_input_digest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const PIPELINE_RECOMMENDATION_SCHEMA: &str = "tect.pipeline-recommendation/3";
pub const PIPELINE_RECOMMENDATION_CONTEXT_SCHEMA: &str = "tect.pipeline-recommendation/4";

/// A projection of the independently evaluated V2 Matrix context. Only the
/// typed composition constructor below can mint this value for a new basis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineMatrixAuthorityBinding {
    frozen_snapshot_id: String,
    requirements_semantic_digest: String,
    authority_schema: String,
    operating_verification_digest: String,
}

impl PipelineMatrixAuthorityBinding {
    pub fn frozen_snapshot_id(&self) -> &str {
        &self.frozen_snapshot_id
    }
    pub fn requirements_semantic_digest(&self) -> &str {
        &self.requirements_semantic_digest
    }
    pub fn authority_schema(&self) -> &str {
        &self.authority_schema
    }
    pub fn operating_verification_digest(&self) -> &str {
        &self.operating_verification_digest
    }

    pub fn validate(&self) -> Result<()> {
        if !uuid::Uuid::parse_str(&self.frozen_snapshot_id)
            .is_ok_and(|id| !id.is_nil() && id.to_string() == self.frozen_snapshot_id)
            || self.authority_schema.trim().is_empty()
            || self.authority_schema.len() > 256
            || !valid_sha256(&self.requirements_semantic_digest)
            || !valid_sha256(&self.operating_verification_digest)
        {
            return Err(Error::StaleContext);
        }
        Ok(())
    }
}

/// Server-loaded Matrix provenance. Current values must be read again before
/// dispatch and disposition; a client-supplied copy has no authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineMatrixBasis {
    pub input: EngineeringMatrixInput,
    pub choice_set: EngineeringChoiceSet,
    pub composition: EngineeringMatrixComposition,
    pub selected_choice_id: String,
    pub current_selected_choice_id: String,
    pub current_task_revision: String,
    pub choice_set_digest: String,
    pub current_choice_set_digest: String,
    pub verification_digest: String,
    pub current_verification_digest: String,
    pub saved_mandatory_card_ids: Vec<String>,
    pub authority: Option<PipelineMatrixAuthorityBinding>,
}

impl PipelineMatrixBasis {
    /// Bind the V2 authority and card projection only from the evaluated
    /// context composition, never from a raw tuple or a legacy status label.
    pub fn with_confirmed_context(
        mut self,
        confirmed: &ContextEngineeringMatrixComposition,
    ) -> Result<Self> {
        if self.authority.is_some()
            || !confirmed.is_resolved()
            || confirmed.composition().task_id != self.composition.task_id
            || confirmed.composition().task_revision != self.composition.task_revision
            || self.verification_digest != confirmed.operating_verification_digest()
            || self.current_verification_digest != confirmed.operating_verification_digest()
        {
            return Err(Error::StaleContext);
        }
        let authority = PipelineMatrixAuthorityBinding {
            frozen_snapshot_id: confirmed.frozen_snapshot_id().to_owned(),
            requirements_semantic_digest: confirmed.requirements_semantic_digest().to_owned(),
            authority_schema: confirmed.authority_schema().to_owned(),
            operating_verification_digest: confirmed.operating_verification_digest().to_owned(),
        };
        authority.validate()?;
        self.composition = confirmed.composition().clone();
        self.authority = Some(authority);
        Ok(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineRecommendationSource {
    pub work: SliceCandidateNode,
    pub current_work_revision: i64,
    pub matrix: PipelineMatrixBasis,
    pub catalogue: PipelineCatalogueSnapshot,
    /// Server-provided current pinned definitions. A missing or invalid kind
    /// is excluded, never repaired or invented by the model.
    pub definitions: Vec<PipelineDefinitionSnapshot>,
    pub compatibility_policy: PipelineCompatibilityPolicy,
    /// Inspectable references only; their presence never records a check pass.
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRecommendationOption {
    /// Stable identity of the exact pipeline and verification-plan pair.
    pub id: String,
    pub kind: PipelineKind,
    pub definition_version: String,
    pub definition_digest: String,
    pub completion_contract: String,
    pub forbidden_claims: Vec<String>,
    pub verification_plan: PipelineVerificationPlan,
}

impl PipelineRecommendationOption {
    pub fn pair_id(kind: PipelineKind, plan_id: &str) -> String {
        format!("{}+{}", kind.as_str(), plan_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRecommendationManifest {
    pub schema: String,
    pub work_id: uuid::Uuid,
    pub work_revision: i64,
    pub matrix_task_id: String,
    pub matrix_task_revision: String,
    pub selected_choice_id: String,
    pub matrix_choice_set_digest: String,
    pub matrix_verification_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix_authority: Option<PipelineMatrixAuthorityBinding>,
    pub matrix_input_digest: String,
    pub selected_candidate_digest: String,
    pub compatibility_policy_digest: String,
    pub mandatory_card_ids: Vec<String>,
    pub deterministic_kind: PipelineKind,
    pub deterministic_option_id: Option<String>,
    pub catalogue_revision: String,
    pub catalogue_digest: String,
    pub options: Vec<PipelineRecommendationOption>,
    pub excluded: Vec<PipelineExcludedKind>,
    pub evidence_refs: Vec<String>,
    pub digest: String,
}

impl PipelineRecommendationManifest {
    pub fn has_bound_v2_authority(&self) -> bool {
        self.schema == PIPELINE_RECOMMENDATION_CONTEXT_SCHEMA
            && self
                .matrix_authority
                .as_ref()
                .is_some_and(|authority| authority.validate().is_ok())
    }

    pub fn should_call(&self) -> bool {
        self.options.len() >= 2
    }

    pub fn validate_digest(&self) -> Result<()> {
        let mut unsigned = self.clone();
        unsigned.digest.clear();
        if !matches!(
            self.schema.as_str(),
            PIPELINE_RECOMMENDATION_SCHEMA | PIPELINE_RECOMMENDATION_CONTEXT_SCHEMA
        ) || (self.schema == PIPELINE_RECOMMENDATION_SCHEMA && self.matrix_authority.is_some())
            || (self.schema == PIPELINE_RECOMMENDATION_CONTEXT_SCHEMA
                && self.matrix_authority.is_none())
            || self
                .matrix_authority
                .as_ref()
                .is_some_and(|binding| binding.validate().is_err())
            || self.work_id.is_nil()
            || self.work_revision < 1
            || self.mandatory_card_ids.is_empty()
            || !valid_sha256(&self.matrix_input_digest)
            || !valid_sha256(&self.selected_candidate_digest)
            || !valid_sha256(&self.compatibility_policy_digest)
            || self.options.len() + self.excluded.len()
                != PipelineKind::CURRENT_SLICE_RUN_KINDS.len()
            || self.excluded.iter().enumerate().any(|(index, excluded)| {
                let order = PipelineKind::CURRENT_SLICE_RUN_KINDS
                    .iter()
                    .position(|kind| *kind == excluded.kind);
                order.is_none()
                    || index > 0
                        && order
                            <= PipelineKind::CURRENT_SLICE_RUN_KINDS
                                .iter()
                                .position(|kind| *kind == self.excluded[index - 1].kind)
            })
            || self.options.iter().any(|option| {
                self.excluded
                    .iter()
                    .any(|excluded| excluded.kind == option.kind)
            })
            || self.options.iter().any(|option| {
                option.verification_plan.validate().is_err()
                    || option.id != Self::pair_id(option)
                    || !PipelineKind::CURRENT_SLICE_RUN_KINDS.contains(&option.kind)
                    || option.definition_version.trim().is_empty()
                    || option.definition_digest.trim().is_empty()
                    || option.verification_plan.source_kind != option.kind
                    || option.verification_plan.source_definition_version
                        != option.definition_version
                    || option.verification_plan.source_definition_digest != option.definition_digest
            })
            || self.deterministic_option_id
                != self
                    .options
                    .iter()
                    .find(|option| option.kind == self.deterministic_kind)
                    .map(|option| option.id.clone())
            || self.options.windows(2).any(|pair| {
                let left = PipelineKind::CURRENT_SLICE_RUN_KINDS
                    .iter()
                    .position(|kind| *kind == pair[0].kind);
                let right = PipelineKind::CURRENT_SLICE_RUN_KINDS
                    .iter()
                    .position(|kind| *kind == pair[1].kind);
                left >= right
            })
            || self
                .options
                .iter()
                .map(|option| &option.id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.options.len()
            || self.digest != digest_json(&unsigned)?
        {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    /// The caller supplies freshly loaded pinned definitions before a saved
    /// recommendation can be used. A valid historical digest is not currentness.
    pub fn validate_against_definitions(
        &self,
        definitions: &[PipelineDefinitionSnapshot],
    ) -> Result<()> {
        self.validate_digest()?;
        for option in &self.options {
            let mut matching = definitions
                .iter()
                .filter(|definition| definition.kind == option.kind);
            let definition = matching.next().ok_or(Error::StaleContext)?;
            if matching.next().is_some()
                || option.definition_version != definition.version
                || option.definition_digest != definition.digest
            {
                return Err(Error::StaleContext);
            }
            option
                .verification_plan
                .validate_against_definition(definition)?;
        }
        Ok(())
    }

    fn pair_id(option: &PipelineRecommendationOption) -> String {
        PipelineRecommendationOption::pair_id(option.kind, &option.verification_plan.id)
    }
}

#[path = "pipeline_recommendation/build.rs"]
mod build;
pub use build::build_pipeline_recommendation_manifest;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineRecommendationRanking {
    Ranked { ranked_ids: Vec<String> },
    Abstained,
}

impl PipelineRecommendationRanking {
    pub fn validate(&self, manifest: &PipelineRecommendationManifest) -> Result<()> {
        manifest.validate_digest()?;
        if !manifest.should_call() {
            return Err(Error::InvalidArguments);
        }
        match self {
            Self::Abstained => Ok(()),
            Self::Ranked { ranked_ids } => {
                let eligible = manifest
                    .options
                    .iter()
                    .map(|option| &option.id)
                    .collect::<BTreeSet<_>>();
                let ranked = ranked_ids.iter().collect::<BTreeSet<_>>();
                if ranked_ids.len() == eligible.len() && ranked == eligible {
                    Ok(())
                } else {
                    Err(Error::InvalidArguments)
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRecommendationDisposition {
    AcceptRecommendation,
    RejectRecommendation,
    UseDeterministicChoice,
}

fn digest_json<T: Serialize>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InvalidArguments)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
#[path = "pipeline_recommendation_tests.rs"]
mod tests;
