use tect_domain::{
    CommitmentEvidence, EngineeringCandidate, EngineeringChoiceSet, EngineeringIntent,
    EngineeringMatrixInput, EngineeringMode, FactProvenance, MATRIX_CHOICE_SET_SCHEMA, MatrixFact,
    OperatingEnvelope, OperatingFact, OperationalFacts, ProtectedGuarantee,
    VerifiedEngineeringMatrixFacts, compose_engineering_matrix, matrix_evaluation_digest,
};

fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("task:42@revision:7".into()),
    }
}

fn input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: known(EngineeringMode::Production),
        envelope: OperatingEnvelope {
            scale: known("synthetic bounded service".into()),
            operational_facts: OperationalFacts::Reported {
                entries: vec![OperatingFact {
                    name: "environment".into(),
                    fact: known("synthetic".into()),
                }],
            },
        },
        criticality: known("payment guarantee".into()),
        intent: known(EngineeringIntent::ProductionHotfix),
        urgency: known("urgent".into()),
        promised_behavior: known("real booking".into()),
        promised_proof: known("booking acceptance".into()),
        affected_guarantees: known(vec![ProtectedGuarantee::Payment]),
        actual_exposure: known(true),
        demand_commitment: known(CommitmentEvidence::LacksEvidence),
        latency_commitment: known(CommitmentEvidence::NoCommitment),
        urgent_repair: known(true),
    }
}

fn choices() -> EngineeringChoiceSet {
    EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "owner-choices".into(),
        version: 1,
        task_id: "task-42".into(),
        task_revision: "revision-7".into(),
        decision_question: "Choose an explicit engineering alternative".into(),
        candidates: ["a", "b"]
            .into_iter()
            .map(|id| EngineeringCandidate {
                candidate_id: id.into(),
                title: format!("Choice {id}"),
                approach: "Owner-authored approach".into(),
                assumption_fact_ids: vec!["mode".into()],
            })
            .collect(),
    }
}

fn compose(input: &EngineeringMatrixInput) -> tect_domain::EngineeringMatrixComposition {
    compose_engineering_matrix(
        &VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
            "task-42".into(),
            "revision-7".into(),
            input.clone(),
        )
        .unwrap(),
    )
}

#[test]
fn full_fresh_composition_digest_rejects_each_activated_card_removal_or_alteration() {
    // Pure shape/digest test only; this constructor is not independent evidence.
    let input = input();
    let fresh = compose(&input);
    assert_eq!(
        fresh
            .mandatory_cards
            .iter()
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        [
            "EM02-SCOPE@0.1",
            "EM02-PROTECT@0.1",
            "EM02-OPERATE@0.1",
            "EM02-CAPACITY@0.1",
            "EM02-HOTFIX@0.1"
        ]
    );
    let set = choices();
    let captured = matrix_evaluation_digest(&input, &fresh, &set)
        .unwrap()
        .unwrap();
    for index in 0..fresh.mandatory_cards.len() {
        let mut removed = fresh.clone();
        removed.mandatory_cards.remove(index);
        assert!(matrix_evaluation_digest(&input, &removed, &set).is_err());
        let mut altered = fresh.clone();
        altered.mandatory_cards[index].body = "weakened card";
        assert!(matrix_evaluation_digest(&input, &altered, &set).is_err());
    }
    let mut changed_choices = set;
    changed_choices.candidates[0].approach.push_str(" changed");
    assert_ne!(
        matrix_evaluation_digest(&input, &fresh, &changed_choices)
            .unwrap()
            .unwrap(),
        captured
    );
}

#[test]
fn no_commitment_does_not_invent_capacity_obligation() {
    let mut input = input();
    input.demand_commitment = known(CommitmentEvidence::NoCommitment);
    input.latency_commitment = known(CommitmentEvidence::NoCommitment);
    let fresh = compose(&input);
    assert!(
        !fresh
            .mandatory_cards
            .iter()
            .any(|card| card.id == "EM02-CAPACITY@0.1")
    );
    assert!(
        matrix_evaluation_digest(&input, &fresh, &choices())
            .unwrap()
            .is_some()
    );
}
