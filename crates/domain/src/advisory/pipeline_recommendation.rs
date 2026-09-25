//! Closed, read-only recommendation material for one saved Work node before `slice.open`.
//! The caller loads current records; this module cannot open a Slice or verify a phase.

use crate::{
    EngineeringChoiceSet, EngineeringMatrixComposition, EngineeringMatrixInput, Error,
    MatrixSourceVerificationStatus, OwnerReportedEngineeringMatrixFacts,
    PIPELINE_RECOMMENDATION_CATALOGUE_REVISION, PipelineCatalogueSnapshot,
    PipelineCompatibilityContext, PipelineCompatibilityPolicy, PipelineDefinitionSnapshot,
    PipelineExcludedKind, PipelineExclusionReason, PipelineExecutionOwner, PipelineKind,
    PipelineVerificationPlan, Result, SliceCandidateNode, VerifiedEngineeringMatrixFacts,
    compose_engineering_matrix, compose_owner_reported_engineering_matrix, matrix_input_digest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const PIPELINE_RECOMMENDATION_SCHEMA: &str = "tect.pipeline-recommendation/3";

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
    pub fn should_call(&self) -> bool {
        self.options.len() >= 2
    }

    pub fn validate_digest(&self) -> Result<()> {
        let mut unsigned = self.clone();
        unsigned.digest.clear();
        if self.schema != PIPELINE_RECOMMENDATION_SCHEMA
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

/// All eight rev4 SliceRun kinds receive a recorded eligibility outcome.
pub fn build_pipeline_recommendation_manifest(
    source: &PipelineRecommendationSource,
) -> Result<PipelineRecommendationManifest> {
    let SliceCandidateNode::Work {
        id,
        revision,
        pipeline,
        ..
    } = &source.work
    else {
        return Err(Error::InvalidArguments);
    };
    if id.is_nil() || *revision < 1 || *revision != source.current_work_revision {
        return Err(Error::StaleRevision);
    }
    let matrix = &source.matrix;
    if matrix.composition.task_id.trim().is_empty()
        || matrix.composition.task_revision.trim().is_empty()
        || matrix.composition.task_revision != matrix.current_task_revision
        || matrix.selected_choice_id.trim().is_empty()
        || matrix.selected_choice_id != matrix.current_selected_choice_id
        || !valid_sha256(&matrix.choice_set_digest)
        || !valid_sha256(&matrix.verification_digest)
        || matrix.choice_set_digest != matrix.current_choice_set_digest
        || matrix.verification_digest != matrix.current_verification_digest
        || !matrix.composition.is_resolved()
    {
        return Err(Error::StaleContext);
    }
    if !matches!(
        matrix.composition.source_verification_status,
        MatrixSourceVerificationStatus::VerifiedByCaller
            | MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
    ) {
        return Err(Error::StaleContext);
    }
    matrix.input.validate()?;
    if matrix.choice_set.task_id != matrix.composition.task_id
        || matrix.choice_set.task_revision != matrix.composition.task_revision
        || matrix.choice_set.canonical_digest(&matrix.input)? != matrix.choice_set_digest
        || !matrix
            .choice_set
            .candidates
            .iter()
            .any(|candidate| candidate.candidate_id == matrix.selected_choice_id)
    {
        return Err(Error::StaleContext);
    }
    let expected_composition = match matrix.composition.source_verification_status {
        MatrixSourceVerificationStatus::VerifiedByCaller => compose_engineering_matrix(
            &VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
                matrix.composition.task_id.clone(),
                matrix.composition.task_revision.clone(),
                matrix.input.clone(),
            )?,
        ),
        MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported => {
            let mut expected = compose_owner_reported_engineering_matrix(
                &OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                    matrix.composition.task_id.clone(),
                    matrix.composition.task_revision.clone(),
                    matrix.input.clone(),
                )?,
            );
            expected.source_verification_status =
                MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported;
            expected
        }
        MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification => {
            return Err(Error::StaleContext);
        }
    };
    if matrix.composition != expected_composition {
        return Err(Error::StaleContext);
    }
    let input_digest = matrix_input_digest(&matrix.input)?;
    let selected_candidate = matrix
        .choice_set
        .candidates
        .iter()
        .find(|candidate| candidate.candidate_id == matrix.selected_choice_id)
        .ok_or(Error::StaleContext)?;
    let selected_candidate_digest = digest_json(selected_candidate)?;
    let mandatory_card_ids = matrix
        .composition
        .mandatory_cards
        .iter()
        .map(|card| card.id.to_string())
        .collect::<BTreeSet<_>>();
    let saved_card_ids = matrix
        .saved_mandatory_card_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if mandatory_card_ids.is_empty()
        || saved_card_ids.len() != matrix.saved_mandatory_card_ids.len()
        || !mandatory_card_ids.is_subset(&saved_card_ids)
    {
        return Err(Error::InvalidArguments);
    }
    if source.catalogue.revision != PIPELINE_RECOMMENDATION_CATALOGUE_REVISION {
        return Err(Error::StaleContext);
    }
    source.catalogue.validate()?;
    let catalogue_kinds = source
        .catalogue
        .entries
        .iter()
        .map(|entry| entry.kind)
        .collect::<BTreeSet<_>>();
    if !PipelineKind::CURRENT_SLICE_RUN_KINDS
        .iter()
        .all(|kind| catalogue_kinds.contains(kind))
    {
        return Err(Error::StaleContext);
    }
    let mut definitions = BTreeMap::new();
    for definition in &source.definitions {
        if definitions.insert(definition.kind, definition).is_some() {
            return Err(Error::InvalidArguments);
        }
    }
    let mut options = Vec::new();
    let mut excluded = Vec::new();
    for kind in PipelineKind::CURRENT_SLICE_RUN_KINDS {
        let entry = source
            .catalogue
            .entries
            .iter()
            .find(|entry| entry.kind == kind)
            .ok_or(Error::StaleContext)?;
        if !entry.executable || entry.execution_owner != PipelineExecutionOwner::SlicePipelineRun {
            excluded.push(PipelineExcludedKind {
                kind,
                reason: PipelineExclusionReason::UnavailableDefinition,
            });
            continue;
        }
        let Some(definition) = definitions.get(&kind) else {
            excluded.push(PipelineExcludedKind {
                kind,
                reason: PipelineExclusionReason::UnavailableDefinition,
            });
            continue;
        };
        if definition.validate().is_err()
            || definition.default_mode
                != entry
                    .default_delivery_mode
                    .unwrap_or(definition.default_mode)
            || definition.allowed_modes != entry.allowed_delivery_modes
        {
            excluded.push(PipelineExcludedKind {
                kind,
                reason: PipelineExclusionReason::UnavailableDefinition,
            });
            continue;
        }
        let verification_plan = PipelineVerificationPlan::from_definition(definition)?;
        let option = PipelineRecommendationOption {
            id: PipelineRecommendationOption::pair_id(kind, &verification_plan.id),
            kind,
            definition_version: definition.version.clone(),
            definition_digest: definition.digest.clone(),
            completion_contract: definition.completion_contract.clone(),
            forbidden_claims: definition.forbidden_claims.clone(),
            verification_plan,
        };
        let context = PipelineCompatibilityContext {
            task_id: &matrix.composition.task_id,
            task_revision: &matrix.composition.task_revision,
            catalogue_revision: &source.catalogue.revision,
            input: &matrix.input,
            input_digest: &input_digest,
            selected_candidate_id: &matrix.selected_choice_id,
            mandatory_cards: &mandatory_card_ids,
        };
        if let Some(reason) = source
            .compatibility_policy
            .reason_for(kind, &context, &option)
        {
            excluded.push(PipelineExcludedKind { kind, reason });
        } else {
            options.push(option);
        }
    }
    if source
        .evidence_refs
        .iter()
        .any(|value| value.trim().is_empty())
        || source.evidence_refs.iter().collect::<BTreeSet<_>>().len() != source.evidence_refs.len()
    {
        return Err(Error::InvalidArguments);
    }
    let mut manifest = PipelineRecommendationManifest {
        schema: PIPELINE_RECOMMENDATION_SCHEMA.into(),
        work_id: *id,
        work_revision: *revision,
        matrix_task_id: matrix.composition.task_id.clone(),
        matrix_task_revision: matrix.composition.task_revision.clone(),
        selected_choice_id: matrix.selected_choice_id.clone(),
        matrix_choice_set_digest: matrix.choice_set_digest.clone(),
        matrix_verification_digest: matrix.verification_digest.clone(),
        matrix_input_digest: input_digest,
        selected_candidate_digest,
        compatibility_policy_digest: source.compatibility_policy.digest()?,
        mandatory_card_ids: mandatory_card_ids.into_iter().collect(),
        deterministic_kind: *pipeline,
        deterministic_option_id: options
            .iter()
            .find(|option| option.kind == *pipeline)
            .map(|option| option.id.clone()),
        catalogue_revision: source.catalogue.revision.clone(),
        catalogue_digest: source.catalogue.digest.clone(),
        options,
        excluded,
        evidence_refs: source
            .evidence_refs
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        digest: String::new(),
    };
    manifest.digest = digest_json(&manifest)?;
    Ok(manifest)
}

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
