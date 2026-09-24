//! Closed, read-only recommendation material for one saved Work node before `slice.open`.
//! The caller loads current records; this module cannot open a Slice or verify a phase.

use crate::{
    EngineeringMatrixComposition, Error, MatrixSourceVerificationStatus,
    PipelineArtifactRequirement, PipelineCatalogueSnapshot, PipelineDefinitionSnapshot,
    PipelineExecutionOwner, PipelineKind, PipelineOutputConstraint, PipelineValidatorContract,
    PipelineVerdictRoute, Result, SliceCandidateNode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const PIPELINE_RECOMMENDATION_SCHEMA: &str = "tect.pipeline-recommendation/1";

/// Server-loaded Matrix provenance. Current values must be read again before
/// dispatch and disposition; a client-supplied copy has no authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineMatrixBasis {
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
    /// Inspectable references only; their presence never records a check pass.
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineVerificationObligation {
    pub phase_id: String,
    pub required_fields: Vec<String>,
    pub required_artifacts: Vec<PipelineArtifactRequirement>,
    pub validator_contracts: Vec<PipelineValidatorContract>,
    pub output_constraints: Vec<PipelineOutputConstraint>,
    pub allowed_verdicts: Vec<String>,
    pub verdict_routes: Vec<PipelineVerdictRoute>,
    pub disposition_required: bool,
    pub required_dispositions: Vec<String>,
    pub fresh_reviewer_input: bool,
    pub output_contract: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRecommendationOption {
    /// Stable ID from `PipelineKind::as_str`, never provider-generated.
    pub id: String,
    pub kind: PipelineKind,
    pub definition_version: String,
    pub definition_digest: String,
    pub completion_contract: String,
    pub forbidden_claims: Vec<String>,
    pub obligations: Vec<PipelineVerificationObligation>,
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
    pub mandatory_card_ids: Vec<String>,
    pub catalogue_revision: String,
    pub catalogue_digest: String,
    pub options: Vec<PipelineRecommendationOption>,
    pub evidence_refs: Vec<String>,
    pub digest: String,
}

impl PipelineRecommendationManifest {
    pub fn should_call(&self) -> bool {
        !self.options.is_empty()
    }

    pub fn validate_digest(&self) -> Result<()> {
        let mut unsigned = self.clone();
        unsigned.digest.clear();
        if self.schema != PIPELINE_RECOMMENDATION_SCHEMA
            || self.work_id.is_nil()
            || self.work_revision < 1
            || self.mandatory_card_ids.is_empty()
            || self.options.iter().any(|option| {
                option.id != option.kind.as_str()
                    || !PipelineKind::CURRENT_SLICE_RUN_KINDS.contains(&option.kind)
                    || option.definition_version.trim().is_empty()
                    || option.definition_digest.trim().is_empty()
            })
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
}

/// All eight rev4 SliceRun kinds are considered. The Matrix's selected choice
/// and mandatory cards are provenance and duties, never a mode-to-kind rule.
pub fn build_pipeline_recommendation_manifest(
    source: &PipelineRecommendationSource,
) -> Result<PipelineRecommendationManifest> {
    let SliceCandidateNode::Work { id, revision, .. } = &source.work else {
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
    if source.catalogue.revision != "4" {
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
    for kind in PipelineKind::CURRENT_SLICE_RUN_KINDS {
        let entry = source
            .catalogue
            .entries
            .iter()
            .find(|entry| entry.kind == kind)
            .ok_or(Error::StaleContext)?;
        if !entry.executable || entry.execution_owner != PipelineExecutionOwner::SlicePipelineRun {
            continue;
        }
        let Some(definition) = definitions.get(&kind) else {
            continue;
        };
        if definition.validate().is_err()
            || definition.default_mode
                != entry
                    .default_delivery_mode
                    .unwrap_or(definition.default_mode)
            || definition.allowed_modes != entry.allowed_delivery_modes
        {
            continue;
        }
        let obligations = definition
            .phases
            .iter()
            .filter(|phase| phase.required)
            .map(|phase| PipelineVerificationObligation {
                phase_id: phase.id.clone(),
                required_fields: phase.required_fields.clone(),
                required_artifacts: phase.required_artifacts.clone(),
                validator_contracts: phase.validator_contracts.clone(),
                output_constraints: phase.output_constraints.clone(),
                allowed_verdicts: phase.allowed_verdicts.clone(),
                verdict_routes: phase.verdict_routes.clone(),
                disposition_required: phase.disposition_required,
                required_dispositions: phase.required_dispositions.clone(),
                fresh_reviewer_input: phase.fresh_reviewer_input,
                output_contract: phase.output_contract.clone(),
            })
            .collect();
        options.push(PipelineRecommendationOption {
            id: kind.as_str().to_string(),
            kind,
            definition_version: definition.version.clone(),
            definition_digest: definition.digest.clone(),
            completion_contract: definition.completion_contract.clone(),
            forbidden_claims: definition.forbidden_claims.clone(),
            obligations,
        });
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
        mandatory_card_ids: mandatory_card_ids.into_iter().collect(),
        catalogue_revision: source.catalogue.revision.clone(),
        catalogue_digest: source.catalogue.digest.clone(),
        options,
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
mod tests {
    use super::*;
    use crate::{
        MandatoryMatrixCard, PipelineCatalogueEntry, PipelineDeliveryMode,
        PipelineInstructionSnapshot, PipelinePhaseDefinition, PipelinePhaseRetryPolicy,
    };
    use uuid::Uuid;

    fn definition(kind: PipelineKind) -> PipelineDefinitionSnapshot {
        let instruction = PipelineInstructionSnapshot {
            id: "instruction".into(),
            version: "1".into(),
            digest: "instruction-digest".into(),
            body: "instruction".into(),
            origin_refs: vec!["source".into()],
        };
        PipelineDefinitionSnapshot {
            kind,
            version: "1".into(),
            digest: format!("definition-{}", kind.as_str()),
            overview: instruction.clone(),
            default_mode: PipelineDeliveryMode::Phasewise,
            allowed_modes: vec![PipelineDeliveryMode::Phasewise],
            phases: vec![PipelinePhaseDefinition {
                id: "proof".into(),
                ordinal: 1,
                title: "Proof".into(),
                required: true,
                disposition_required: false,
                instructions: vec![instruction],
                skills: vec![],
                resources: vec![],
                required_artifacts: vec![PipelineArtifactRequirement {
                    name_pattern: "proof.txt".into(),
                    media_type: "text/plain".into(),
                    schema_ref: None,
                    schema_resource_id: None,
                    schema_resource_digest: None,
                    required: true,
                    minimum_matches: 1,
                    when_verdict: None,
                }],
                validator_contracts: vec![],
                required_fields: vec!["proof_summary".into()],
                allowed_verdicts: vec![],
                required_dispositions: vec![],
                allowed_dispositions: vec![],
                output_constraints: vec![],
                verdict_routes: vec![],
                followup_contracts: vec![],
                allowed_backward_to: vec![],
                fresh_reviewer_input: false,
                retry_policy: PipelinePhaseRetryPolicy::Repeatable,
                output_contract: "proof is checked by the caller".into(),
            }],
            completion_contract: "completion proof".into(),
            escalation_contract: "escalation".into(),
            forbidden_claims: vec!["unverified".into()],
        }
    }

    fn source() -> PipelineRecommendationSource {
        let kinds = PipelineKind::CURRENT_SLICE_RUN_KINDS
            .into_iter()
            .chain([PipelineKind::PromoteToDurableKnowledge]);
        let entries = kinds
            .map(|kind| PipelineCatalogueEntry {
                kind,
                description: "description".into(),
                implementation_status: "executable".into(),
                description_status: "refined".into(),
                refinement_required: false,
                choose_when: "choose".into(),
                do_not_choose_when: "avoid".into(),
                expected_result: "result".into(),
                executable: true,
                default_delivery_mode: Some(PipelineDeliveryMode::Phasewise),
                allowed_delivery_modes: vec![PipelineDeliveryMode::Phasewise],
                execution_owner: if kind == PipelineKind::PromoteToDurableKnowledge {
                    PipelineExecutionOwner::KnowledgeChange
                } else {
                    PipelineExecutionOwner::SlicePipelineRun
                },
            })
            .collect();
        let card = MandatoryMatrixCard {
            id: "EM02-SCOPE@0.1",
            catalogue_version: "EM02-INITIAL@0.1",
            summary: "scope",
            body: "scope proof",
        };
        PipelineRecommendationSource {
            work: SliceCandidateNode::Work {
                id: Uuid::new_v4(),
                revision: 2,
                title: "work".into(),
                outcome: "outcome".into(),
                includes: vec![],
                excludes: vec![],
                dependencies: vec![],
                proof: vec![],
                pipeline: PipelineKind::LightweightTddDevelopment,
                pipeline_reason: "existing choice".into(),
                why_lightweight_insufficient: None,
                why_further_vertical_split_not_viable: None,
                source_result_ids: vec![],
                source_checkpoint: None,
            },
            current_work_revision: 2,
            matrix: PipelineMatrixBasis {
                composition: EngineeringMatrixComposition {
                    catalogue_version: "EM02-INITIAL@0.1",
                    task_id: "task".into(),
                    task_revision: "3".into(),
                    source_verification_status: MatrixSourceVerificationStatus::VerifiedByCaller,
                    mandatory_cards: vec![card],
                    unresolved_evidence: vec![],
                },
                selected_choice_id: "choice".into(),
                current_selected_choice_id: "choice".into(),
                current_task_revision: "3".into(),
                choice_set_digest: "a".repeat(64),
                current_choice_set_digest: "a".repeat(64),
                verification_digest: "b".repeat(64),
                current_verification_digest: "b".repeat(64),
                saved_mandatory_card_ids: vec!["EM02-SCOPE@0.1".into()],
            },
            catalogue: PipelineCatalogueSnapshot {
                revision: "4".into(),
                digest: "catalogue".into(),
                entries,
            },
            definitions: PipelineKind::CURRENT_SLICE_RUN_KINDS
                .into_iter()
                .map(definition)
                .collect(),
            evidence_refs: vec!["evidence:2".into(), "evidence:1".into()],
        }
    }

    #[test]
    fn current_eight_are_closed_and_promotion_is_excluded() {
        let manifest = build_pipeline_recommendation_manifest(&source()).unwrap();
        assert!(manifest.should_call());
        assert_eq!(manifest.options.len(), 8);
        assert!(
            !manifest
                .options
                .iter()
                .any(|option| option.kind == PipelineKind::PromoteToDurableKnowledge)
        );
        assert_eq!(
            manifest.options[0].id,
            PipelineKind::LightweightTddDevelopment.as_str()
        );
        assert_eq!(
            manifest.options[0].obligations[0].required_fields,
            ["proof_summary"]
        );
        assert_eq!(
            manifest.options[0].obligations[0].required_artifacts[0].name_pattern,
            "proof.txt"
        );
        assert_eq!(manifest.mandatory_card_ids, ["EM02-SCOPE@0.1"]);
        assert_eq!(manifest.evidence_refs, ["evidence:1", "evidence:2"]);
        manifest.validate_digest().unwrap();
    }

    #[test]
    fn digest_is_canonical_for_definition_and_evidence_order_and_binds_content() {
        let original = source();
        let first = build_pipeline_recommendation_manifest(&original).unwrap();
        let mut reordered = original.clone();
        reordered.definitions.reverse();
        reordered.evidence_refs.reverse();
        assert_eq!(
            first.digest,
            build_pipeline_recommendation_manifest(&reordered)
                .unwrap()
                .digest
        );
        reordered.definitions[0].digest.push('x');
        assert_ne!(
            first.digest,
            build_pipeline_recommendation_manifest(&reordered)
                .unwrap()
                .digest
        );
        let mut tampered = first;
        tampered.mandatory_card_ids.clear();
        assert_eq!(tampered.validate_digest(), Err(Error::InputConflict));
    }

    #[test]
    fn stale_unresolved_or_missing_matrix_duties_reject_before_call() {
        let mut stale = source();
        stale.current_work_revision += 1;
        assert_eq!(
            build_pipeline_recommendation_manifest(&stale),
            Err(Error::StaleRevision)
        );
        stale = source();
        stale.matrix.current_task_revision = "4".into();
        assert_eq!(
            build_pipeline_recommendation_manifest(&stale),
            Err(Error::StaleContext)
        );
        stale = source();
        stale.matrix.current_selected_choice_id = "other".into();
        assert_eq!(
            build_pipeline_recommendation_manifest(&stale),
            Err(Error::StaleContext)
        );
        stale = source();
        stale.matrix.current_verification_digest = "c".repeat(64);
        assert_eq!(
            build_pipeline_recommendation_manifest(&stale),
            Err(Error::StaleContext)
        );
        stale = source();
        stale.matrix.composition.source_verification_status =
            MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification;
        assert_eq!(
            build_pipeline_recommendation_manifest(&stale),
            Err(Error::StaleContext)
        );
        stale = source();
        stale.matrix.saved_mandatory_card_ids.clear();
        assert_eq!(
            build_pipeline_recommendation_manifest(&stale),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn invalid_definitions_are_ineligible_and_empty_set_means_no_call() {
        let mut input = source();
        input.definitions[0].allowed_modes = vec![PipelineDeliveryMode::Whole];
        let manifest = build_pipeline_recommendation_manifest(&input).unwrap();
        assert_eq!(manifest.options.len(), 7);
        input.definitions.clear();
        let empty = build_pipeline_recommendation_manifest(&input).unwrap();
        assert!(!empty.should_call());
        assert_eq!(
            PipelineRecommendationRanking::Abstained.validate(&empty),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn ranking_requires_exact_permutation_or_abstention() {
        let manifest = build_pipeline_recommendation_manifest(&source()).unwrap();
        let ids = manifest
            .options
            .iter()
            .map(|option| option.id.clone())
            .collect::<Vec<_>>();
        assert!(
            PipelineRecommendationRanking::Ranked {
                ranked_ids: ids.clone()
            }
            .validate(&manifest)
            .is_ok()
        );
        assert!(
            PipelineRecommendationRanking::Abstained
                .validate(&manifest)
                .is_ok()
        );
        for ranked_ids in [
            ids[..7].to_vec(),
            vec![ids[0].clone(); 8],
            [ids[..7].to_vec(), vec!["unknown".into()]].concat(),
        ] {
            assert_eq!(
                PipelineRecommendationRanking::Ranked { ranked_ids }.validate(&manifest),
                Err(Error::InvalidArguments)
            );
        }
    }
}
