use super::*;
use crate::{
    CommitmentEvidence, EngineeringCandidate, EngineeringIntent, EngineeringMode, FactProvenance,
    MATRIX_CHOICE_SET_SCHEMA, MatrixFact, OperatingEnvelope, OperationalFacts,
    PIPELINE_COMPATIBILITY_POLICY_VERSION, PipelineCardCoverage, PipelineCatalogueEntry,
    PipelineCompatibilityRule, PipelineDeliveryMode, PipelineInstructionSnapshot,
    PipelinePhaseDefinition, PipelinePhaseRetryPolicy, ProtectedGuarantee,
    pipeline_obligation_digest,
};
use uuid::Uuid;

fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("source".into()),
    }
}

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
    let input = EngineeringMatrixInput {
        mode: known(EngineeringMode::Demo),
        envelope: OperatingEnvelope {
            scale: known("observed".into()),
            operational_facts: OperationalFacts::KnownEmpty {
                provenance: FactProvenance("source".into()),
            },
        },
        criticality: known("low".into()),
        intent: known(EngineeringIntent::Other("new work".into())),
        urgency: known("routine".into()),
        promised_behavior: known("works".into()),
        promised_proof: known("proof".into()),
        affected_guarantees: MatrixFact::<Vec<ProtectedGuarantee>>::KnownEmpty {
            provenance: FactProvenance("source".into()),
        },
        actual_exposure: known(false),
        demand_commitment: known(CommitmentEvidence::NoCommitment),
        latency_commitment: known(CommitmentEvidence::NoCommitment),
        urgent_repair: known(false),
    };
    let composition = compose_engineering_matrix(
        &VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
            "task".into(),
            "3".into(),
            input.clone(),
        )
        .unwrap(),
    );
    let choice_set = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "set".into(),
        version: 1,
        task_id: "task".into(),
        task_revision: "3".into(),
        decision_question: "approach?".into(),
        candidates: ["choice", "other"]
            .map(|id| EngineeringCandidate {
                candidate_id: id.into(),
                title: id.into(),
                approach: "approach".into(),
                assumption_fact_ids: vec![],
            })
            .to_vec(),
    };
    let choice_set_digest = choice_set.canonical_digest(&input).unwrap();
    let input_digest = matrix_input_digest(&input).unwrap();
    let obligation = {
        let phase = definition(PipelineKind::LightweightTddDevelopment)
            .phases
            .remove(0);
        PipelineVerificationObligation {
            phase_id: phase.id,
            required_fields: phase.required_fields,
            required_artifacts: phase.required_artifacts,
            validator_contracts: phase.validator_contracts,
            output_constraints: phase.output_constraints,
            allowed_verdicts: phase.allowed_verdicts,
            verdict_routes: phase.verdict_routes,
            disposition_required: phase.disposition_required,
            required_dispositions: phase.required_dispositions,
            fresh_reviewer_input: phase.fresh_reviewer_input,
            output_contract: phase.output_contract,
        }
    };
    let policy = PipelineCompatibilityPolicy {
        version: PIPELINE_COMPATIBILITY_POLICY_VERSION.into(),
        rules: PipelineKind::CURRENT_SLICE_RUN_KINDS
            .into_iter()
            .map(|kind| PipelineCompatibilityRule {
                kind,
                matrix_input_digest: input_digest.clone(),
                allowed_modes: vec![EngineeringMode::Demo],
                selected_candidate_ids: vec!["choice".into()],
                card_coverage: vec![PipelineCardCoverage {
                    card_id: "EM02-SCOPE@0.1".into(),
                    phase_id: "proof".into(),
                    obligation_digest: pipeline_obligation_digest(&obligation).unwrap(),
                }],
            })
            .collect(),
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
            input,
            choice_set,
            composition,
            selected_choice_id: "choice".into(),
            current_selected_choice_id: "choice".into(),
            current_task_revision: "3".into(),
            choice_set_digest: choice_set_digest.clone(),
            current_choice_set_digest: choice_set_digest,
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
        compatibility_policy: policy,
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
    assert_eq!(empty.excluded.len(), 8);
    assert_eq!(
        PipelineRecommendationRanking::Abstained.validate(&empty),
        Err(Error::InvalidArguments)
    );
}

#[path = "pipeline_recommendation_compatibility_tests.rs"]
mod compatibility_tests;

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
