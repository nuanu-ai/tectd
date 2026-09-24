use super::*;
use crate::{
    EngineeringMatrixInput, MatrixFact, OperatingEnvelope, OwnerReportedEngineeringMatrixFacts,
    compose_owner_reported_engineering_matrix,
};

fn input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: MatrixFact::Absent,
            operational_facts: OperationalFacts::Absent,
        },
        criticality: MatrixFact::Absent,
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        affected_guarantees: MatrixFact::Absent,
        actual_exposure: MatrixFact::Absent,
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: MatrixFact::Absent,
    }
}

fn candidate(id: &str) -> EngineeringCandidate {
    EngineeringCandidate {
        candidate_id: id.into(),
        title: format!("Alternative {id}"),
        approach: "Deliver the promised behavior".into(),
        assumption_fact_ids: vec!["criticality".into()],
    }
}

fn choices(ids: &[&str]) -> EngineeringChoiceSet {
    EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "choice-1".into(),
        version: 1,
        task_id: "task-1".into(),
        task_revision: "7".into(),
        decision_question: "Which approach best meets the stated behavior?".into(),
        candidates: ids.iter().map(|id| candidate(id)).collect(),
    }
}

#[test]
fn zero_and_one_are_recordable_but_not_rankable() {
    for ids in [&[][..], &["a"][..]] {
        let set = choices(ids);
        assert_eq!(
            set.validate(&input()).unwrap(),
            MatrixAdviceEligibility::NotApplicable
        );
        let composition = compose_owner_reported_engineering_matrix(
            &OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                "task-1".into(),
                "7".into(),
                input(),
            )
            .unwrap(),
        );
        assert_eq!(
            matrix_evaluation_digest(&input(), &composition, &set).unwrap(),
            None
        );
    }
}

#[test]
fn candidate_bounds_fact_references_and_identity_are_enforced() {
    let mut set = choices(&["a", "b"]);
    assert!(matches!(
        set.validate(&input()),
        Ok(MatrixAdviceEligibility::EligibleForAdvice { .. })
    ));
    set.candidates[1].candidate_id = "a".into();
    assert_eq!(set.validate(&input()), Err(Error::InvalidArguments));
    set.candidates[1].candidate_id = "b".into();
    set.candidates[1].assumption_fact_ids = vec!["EM02-SCOPE@0.1".into()];
    assert_eq!(set.validate(&input()), Err(Error::InvalidArguments));
    set.candidates[1].assumption_fact_ids.clear();
    set.candidates.extend([
        candidate("c"),
        candidate("d"),
        candidate("e"),
        candidate("f"),
    ]);
    assert_eq!(set.validate(&input()), Err(Error::InvalidArguments));
}

#[test]
fn canonical_digest_is_order_independent_and_binding_changes_with_facts() {
    let mut left = choices(&["b", "a"]);
    let mut right = choices(&["a", "b"]);
    left.candidates[0]
        .assumption_fact_ids
        .push("urgency".into());
    right.candidates[1]
        .assumption_fact_ids
        .insert(0, "urgency".into());
    assert_eq!(
        left.canonical_digest(&input()).unwrap(),
        right.canonical_digest(&input()).unwrap()
    );
    let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        "task-1".into(),
        "7".into(),
        input(),
    )
    .unwrap();
    let composition = compose_owner_reported_engineering_matrix(&reported);
    let digest = matrix_evaluation_digest(&input(), &composition, &left)
        .unwrap()
        .unwrap();
    let mut changed = input();
    changed.criticality = MatrixFact::Unknown {
        provenance: crate::FactProvenance("owner-revision-7".into()),
    };
    let changed_composition = compose_owner_reported_engineering_matrix(
        &OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
            "task-1".into(),
            "7".into(),
            changed.clone(),
        )
        .unwrap(),
    );
    assert_ne!(
        digest,
        matrix_evaluation_digest(&changed, &changed_composition, &left)
            .unwrap()
            .unwrap()
    );
    let mut stale = left.clone();
    stale.task_revision = "8".into();
    assert_eq!(
        matrix_evaluation_digest(&input(), &composition, &stale),
        Err(Error::StaleRevision)
    );
}

#[test]
fn evaluation_rejects_changed_catalogue_or_forged_same_revision_composition() {
    let facts = input();
    let set = choices(&["a", "b"]);
    let composition = compose_owner_reported_engineering_matrix(
        &OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
            "task-1".into(),
            "7".into(),
            facts.clone(),
        )
        .unwrap(),
    );
    let digest = matrix_evaluation_digest(&facts, &composition, &set)
        .unwrap()
        .unwrap();
    // Explicitly pin the catalogue version in the hash payload, even when a
    // policy update happens to leave the current applicable card list intact.
    let old_payload_without_catalogue = sha256_json(&(
        MATRIX_EVALUATION_CONTRACT_VERSION,
        &composition.task_id,
        &composition.task_revision,
        &facts,
        &composition.source_verification_status,
        &composition.mandatory_cards,
        &composition.unresolved_evidence,
        set.canonical_digest(&facts).unwrap(),
    ))
    .unwrap();
    assert_ne!(digest, old_payload_without_catalogue);

    let mut forged = composition.clone();
    forged.catalogue_version = "EM02-INITIAL@changed";
    assert_eq!(
        matrix_evaluation_digest(&facts, &forged, &set),
        Err(Error::InvalidArguments)
    );
    let mut forged = composition.clone();
    forged.mandatory_cards.clear();
    assert_eq!(
        matrix_evaluation_digest(&facts, &forged, &set),
        Err(Error::InvalidArguments)
    );
    let mut forged = composition.clone();
    forged.unresolved_evidence.clear();
    assert_eq!(
        matrix_evaluation_digest(&facts, &forged, &set),
        Err(Error::InvalidArguments)
    );
    // Pending owner facts remain pending; recomposition does not upgrade them.
    assert!(matrix_evaluation_digest(&facts, &composition, &set).is_ok());
}

#[test]
fn provider_rank_must_be_exact_permutation_or_empty_abstention() {
    let eligible = choices(&["a", "b", "c"]).validate(&input()).unwrap();
    let ranked = |ids: &[&str], recommended: &str| MatrixRanking::Ranked {
        ranked_candidate_ids: ids.iter().map(|id| (*id).into()).collect(),
        recommended_candidate_id: recommended.into(),
    };
    assert!(ranked(&["b", "a", "c"], "b").validate(&eligible).is_ok());
    for invalid in [
        ranked(&["a", "b"], "a"),
        ranked(&["a", "b", "b"], "a"),
        ranked(&["a", "b", "alien"], "a"),
        ranked(&["a", "b", "c"], "b"),
        MatrixRanking::Abstained {
            ranked_candidate_ids: vec!["a".into()],
            recommended_candidate_id: None,
        },
        MatrixRanking::Abstained {
            ranked_candidate_ids: vec![],
            recommended_candidate_id: Some("a".into()),
        },
    ] {
        assert_eq!(invalid.validate(&eligible), Err(Error::InvalidArguments));
    }
    assert!(
        MatrixRanking::Abstained {
            ranked_candidate_ids: vec![],
            recommended_candidate_id: None,
        }
        .validate(&eligible)
        .is_ok()
    );
    assert_eq!(
        ranked(&["a", "b", "c"], "a").validate(&MatrixAdviceEligibility::NotApplicable),
        Err(Error::InvalidArguments)
    );
}
