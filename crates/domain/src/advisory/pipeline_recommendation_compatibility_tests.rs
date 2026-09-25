use super::*;
#[test]
fn all_matrix_cards_need_explicit_required_phase_coverage() {
    let mut input = source();
    input.matrix.input.mode = known(EngineeringMode::Production);
    input.matrix.input.intent = known(EngineeringIntent::ProductionHotfix);
    input.matrix.input.affected_guarantees = known(vec![ProtectedGuarantee::Payment]);
    input.matrix.input.actual_exposure = known(true);
    input.matrix.input.demand_commitment = known(CommitmentEvidence::LacksEvidence);
    input.matrix.input.urgent_repair = known(true);
    input.matrix.composition = compose_engineering_matrix(
        &VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
            "task".into(),
            "3".into(),
            input.matrix.input.clone(),
        )
        .unwrap(),
    );
    let cards = input
        .matrix
        .composition
        .mandatory_cards
        .iter()
        .map(|card| card.id.to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        cards,
        [
            "EM02-SCOPE@0.1",
            "EM02-PROTECT@0.1",
            "EM02-OPERATE@0.1",
            "EM02-CAPACITY@0.1",
            "EM02-HOTFIX@0.1",
        ]
    );
    input.matrix.saved_mandatory_card_ids = cards.clone();
    let choice_digest = input
        .matrix
        .choice_set
        .canonical_digest(&input.matrix.input)
        .unwrap();
    input.matrix.choice_set_digest = choice_digest.clone();
    input.matrix.current_choice_set_digest = choice_digest;
    let input_digest = matrix_input_digest(&input.matrix.input).unwrap();
    for rule in &mut input.compatibility_policy.rules {
        rule.matrix_input_digest = input_digest.clone();
        rule.allowed_modes = vec![EngineeringMode::Production];
    }
    let missing = build_pipeline_recommendation_manifest(&input).unwrap();
    assert!(missing.options.is_empty());
    assert!(
        missing
            .excluded
            .iter()
            .all(|kind| kind.reason == PipelineExclusionReason::IncompleteCardCoverage)
    );

    let template = input.compatibility_policy.rules[0].card_coverage[0].clone();
    for rule in &mut input.compatibility_policy.rules {
        rule.card_coverage = cards
            .iter()
            .map(|card_id| PipelineCardCoverage {
                card_id: card_id.clone(),
                ..template.clone()
            })
            .collect();
    }
    assert_eq!(
        build_pipeline_recommendation_manifest(&input)
            .unwrap()
            .options
            .len(),
        8
    );
    for card_id in &cards {
        let mut missing_one = input.clone();
        missing_one.compatibility_policy.rules[0]
            .card_coverage
            .retain(|coverage| &coverage.card_id != card_id);
        let manifest = build_pipeline_recommendation_manifest(&missing_one).unwrap();
        assert_eq!(manifest.options.len(), 7, "{card_id}");
        assert_eq!(
            manifest.excluded[0].reason,
            PipelineExclusionReason::IncompleteCardCoverage
        );
    }
    input.compatibility_policy.rules[0].card_coverage[0].phase_id = "nonexistent".into();
    assert_eq!(
        build_pipeline_recommendation_manifest(&input)
            .unwrap()
            .excluded[0]
            .reason,
        PipelineExclusionReason::MissingPhaseObligation
    );
}

#[test]
fn unknown_stale_and_single_option_policy_fail_closed() {
    let mut input = source();
    input.compatibility_policy = PipelineCompatibilityPolicy::unavailable();
    let unavailable = build_pipeline_recommendation_manifest(&input).unwrap();
    assert!(unavailable.options.is_empty());
    assert!(!unavailable.should_call());

    input = source();
    input.compatibility_policy.version = "unknown".into();
    let manifest = build_pipeline_recommendation_manifest(&input).unwrap();
    assert_eq!(manifest.options.len(), 0);
    assert!(
        manifest
            .excluded
            .iter()
            .all(|kind| kind.reason == PipelineExclusionReason::UnsupportedPolicy)
    );

    input = source();
    input.compatibility_policy.task_revision = "stale".into();
    assert_eq!(
        build_pipeline_recommendation_manifest(&input)
            .unwrap()
            .excluded[0]
            .reason,
        PipelineExclusionReason::StaleTask
    );
    input = source();
    input.compatibility_policy.catalogue_revision = "stale".into();
    assert_eq!(
        build_pipeline_recommendation_manifest(&input)
            .unwrap()
            .excluded[0]
            .reason,
        PipelineExclusionReason::StaleCatalogue
    );
    input = source();
    input.compatibility_policy.rules[0].matrix_input_digest = "f".repeat(64);
    assert_eq!(
        build_pipeline_recommendation_manifest(&input)
            .unwrap()
            .excluded[0]
            .reason,
        PipelineExclusionReason::StaleMatrixInput
    );
    input = source();
    input.compatibility_policy.rules[0].selected_candidate_ids = vec!["other".into()];
    assert_eq!(
        build_pipeline_recommendation_manifest(&input)
            .unwrap()
            .excluded[0]
            .reason,
        PipelineExclusionReason::IncompatibleCandidate
    );

    input = source();
    input.compatibility_policy.rules.truncate(1);
    let one = build_pipeline_recommendation_manifest(&input).unwrap();
    assert_eq!(one.options.len(), 1);
    assert!(!one.should_call());
    assert!(
        one.excluded
            .iter()
            .all(|kind| kind.reason == PipelineExclusionReason::MissingRule)
    );
    assert_eq!(
        PipelineRecommendationRanking::Abstained.validate(&one),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn legacy_unpinned_snapshot_decodes_but_cannot_become_eligible() {
    let mut legacy = serde_json::to_value(source().compatibility_policy).unwrap();
    for field in ["task_id", "task_revision", "catalogue_revision"] {
        legacy.as_object_mut().unwrap().remove(field);
    }
    let policy: PipelineCompatibilityPolicy = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(serde_json::to_value(&policy).unwrap(), legacy);
    let mut input = source();
    input.compatibility_policy = policy;
    let manifest = build_pipeline_recommendation_manifest(&input).unwrap();
    assert!(manifest.options.is_empty());
    assert!(
        manifest
            .excluded
            .iter()
            .all(|excluded| excluded.reason == PipelineExclusionReason::StaleTask)
    );
}
