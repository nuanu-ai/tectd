use super::*;
use crate::{EngineeringMode, OperatingEnvelope, OperatingFact, ProtectedGuarantee};

fn source() -> FactProvenance {
    FactProvenance("task:42@revision:7".into())
}

fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: source(),
    }
}

fn facts(mode: EngineeringMode) -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: known(mode),
        envelope: OperatingEnvelope {
            scale: known("synthetic request with no demand promise".into()),
            operational_facts: OperationalFacts::Reported {
                entries: vec![OperatingFact {
                    name: "environment".into(),
                    fact: known("synthetic".into()),
                }],
            },
        },
        criticality: known("no protected guarantee affected".into()),
        intent: known(EngineeringIntent::Other("booking flow".into())),
        urgency: known("ordinary".into()),
        promised_behavior: known("real booking".into()),
        promised_proof: known("booking acceptance".into()),
        affected_guarantees: MatrixFact::KnownEmpty {
            provenance: source(),
        },
        actual_exposure: known(false),
        demand_commitment: known(CommitmentEvidence::NoCommitment),
        latency_commitment: known(CommitmentEvidence::NoCommitment),
        urgent_repair: known(false),
    }
}

fn compose(input: EngineeringMatrixInput) -> EngineeringMatrixComposition {
    let verified = VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
        "task-42".into(),
        "revision-7".into(),
        input,
    )
    .unwrap();
    compose_engineering_matrix(&verified)
}

fn compose_reported(input: EngineeringMatrixInput) -> EngineeringMatrixComposition {
    let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        "task-42".into(),
        "revision-7".into(),
        input,
    )
    .unwrap();
    compose_owner_reported_engineering_matrix(&reported)
}

fn ids(output: &EngineeringMatrixComposition) -> Vec<&str> {
    output.mandatory_cards.iter().map(|card| card.id).collect()
}

#[test]
fn demo_scope_only_is_resolved_and_does_not_infer_scale() {
    let output = compose(facts(EngineeringMode::Demo));
    assert_eq!(ids(&output), ["EM02-SCOPE@0.1"]);
    assert!(output.is_resolved());
    assert!(
        output.mandatory_cards[0]
            .body
            .contains("fake external behavior requires explicit operator approval")
    );
}

#[test]
fn fully_reported_demo_is_provisional_even_with_no_field_gaps() {
    let output = compose_reported(facts(EngineeringMode::Demo));
    assert_eq!(ids(&output), ["EM02-SCOPE@0.1"]);
    assert!(output.unresolved_evidence.is_empty());
    assert_eq!(
        output.source_verification_status,
        MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification
    );
    assert!(!output.is_resolved());
}

#[test]
fn reported_production_triggers_keep_all_mandatory_cards_while_pending() {
    let mut input = facts(EngineeringMode::Production);
    input.intent = known(EngineeringIntent::ProductionHotfix);
    input.urgency = known("urgent".into());
    input.urgent_repair = known(true);
    input.affected_guarantees = known(vec![ProtectedGuarantee::Payment]);
    input.actual_exposure = known(true);
    input.demand_commitment = known(CommitmentEvidence::LacksEvidence);
    let output = compose_reported(input);
    assert_eq!(
        ids(&output),
        [
            "EM02-SCOPE@0.1",
            "EM02-PROTECT@0.1",
            "EM02-OPERATE@0.1",
            "EM02-CAPACITY@0.1",
            "EM02-HOTFIX@0.1"
        ]
    );
    assert!(!output.is_resolved());
}

#[test]
fn real_payment_mvp_keeps_protection_and_operations() {
    let mut input = facts(EngineeringMode::Mvp);
    input.affected_guarantees = known(vec![
        ProtectedGuarantee::Payment,
        ProtectedGuarantee::Secret,
        ProtectedGuarantee::Data,
    ]);
    input.actual_exposure = known(true);
    let output = compose(input);
    assert_eq!(
        ids(&output),
        ["EM02-SCOPE@0.1", "EM02-PROTECT@0.1", "EM02-OPERATE@0.1"]
    );
    assert!(output.is_resolved());
    assert!(output.mandatory_cards[1].body.contains("every mode"));
}

#[test]
fn capacity_depends_on_commitment_evidence_not_production_label() {
    let mut input = facts(EngineeringMode::Production);
    input.actual_exposure = known(true);
    input.demand_commitment = known(CommitmentEvidence::ExceedsVerifiedLimit);
    let output = compose(input.clone());
    assert_eq!(
        ids(&output),
        ["EM02-SCOPE@0.1", "EM02-OPERATE@0.1", "EM02-CAPACITY@0.1"]
    );
    input.demand_commitment = known(CommitmentEvidence::WithinVerifiedLimit);
    assert_eq!(ids(&compose(input)), ["EM02-SCOPE@0.1", "EM02-OPERATE@0.1"]);
    let mut demo = facts(EngineeringMode::Demo);
    demo.latency_commitment = known(CommitmentEvidence::LacksEvidence);
    assert_eq!(ids(&compose(demo)), ["EM02-SCOPE@0.1", "EM02-CAPACITY@0.1"]);
}

#[test]
fn urgent_production_hotfix_keeps_existing_obligations() {
    let mut input = facts(EngineeringMode::Production);
    input.intent = known(EngineeringIntent::ProductionHotfix);
    input.urgency = known("urgent".into());
    input.urgent_repair = known(true);
    input.affected_guarantees = known(vec![ProtectedGuarantee::Payment]);
    input.actual_exposure = known(true);
    input.latency_commitment = known(CommitmentEvidence::LacksEvidence);
    let output = compose(input);
    assert_eq!(
        ids(&output),
        [
            "EM02-SCOPE@0.1",
            "EM02-PROTECT@0.1",
            "EM02-OPERATE@0.1",
            "EM02-CAPACITY@0.1",
            "EM02-HOTFIX@0.1"
        ]
    );
    assert!(output.is_resolved());
    assert!(
        output.mandatory_cards[4]
            .body
            .contains("Existing duties remain")
    );
}

#[test]
fn missing_uncertain_and_invalid_triggers_stay_explicit() {
    let mut input = facts(EngineeringMode::Demo);
    input.affected_guarantees = MatrixFact::Absent;
    input.actual_exposure = MatrixFact::Unknown {
        provenance: source(),
    };
    input.demand_commitment = MatrixFact::Conflict {
        provenance: source(),
    };
    input.latency_commitment = MatrixFact::Invalid {
        provenance: source(),
    };
    input.promised_proof = MatrixFact::Gap {
        provenance: source(),
    };
    let output = compose(input);
    assert_eq!(ids(&output), ["EM02-SCOPE@0.1"]);
    assert!(!output.is_resolved());
    for (field, state) in [
        ("affected_guarantees", MatrixEvidenceState::Absent),
        ("actual_exposure", MatrixEvidenceState::Unknown),
        ("demand_commitment", MatrixEvidenceState::Conflict),
        ("latency_commitment", MatrixEvidenceState::Invalid),
        ("promised_proof", MatrixEvidenceState::Gap),
    ] {
        assert!(
            output
                .unresolved_evidence
                .iter()
                .any(|issue| issue.field == field && issue.state == state)
        );
    }
}

#[test]
fn partial_capacity_evidence_does_not_drop_established_capacity_duty() {
    let mut input = facts(EngineeringMode::Production);
    input.demand_commitment = known(CommitmentEvidence::ExceedsVerifiedLimit);
    input.latency_commitment = MatrixFact::Unknown {
        provenance: source(),
    };
    let output = compose(input);
    assert_eq!(ids(&output), ["EM02-SCOPE@0.1", "EM02-CAPACITY@0.1"]);
    assert!(!output.is_resolved());
}

#[test]
fn contradictory_hotfix_evidence_is_unresolved() {
    let mut input = facts(EngineeringMode::Production);
    input.intent = known(EngineeringIntent::ProductionHotfix);
    input.urgent_repair = known(false);
    let output = compose(input);
    assert_eq!(ids(&output), ["EM02-SCOPE@0.1"]);
    assert!(output.unresolved_evidence.iter().any(
        |issue| issue.field == "urgent_repair" && issue.state == MatrixEvidenceState::Conflict
    ));
}

#[test]
fn urgent_production_repair_keeps_hotfix_card_despite_other_intent() {
    let mut input = facts(EngineeringMode::Production);
    input.urgency = known("ordinary sequencing".into());
    input.urgent_repair = known(true);
    let output = compose(input);
    assert_eq!(ids(&output), ["EM02-SCOPE@0.1", "EM02-HOTFIX@0.1"]);
    assert!(!output.is_resolved());
    assert!(
        output
            .unresolved_evidence
            .iter()
            .any(|issue| issue.field == "intent" && issue.state == MatrixEvidenceState::Conflict)
    );
}

#[test]
fn empty_operational_fact_and_missing_mode_stay_visible() {
    let mut input = facts(EngineeringMode::Demo);
    input.mode = MatrixFact::Absent;
    input.envelope.operational_facts = OperationalFacts::Absent;
    let output = compose(input);
    assert!(!output.is_resolved());
    assert!(
        output
            .unresolved_evidence
            .iter()
            .any(|issue| issue.field == "mode")
    );
    assert!(
        output
            .unresolved_evidence
            .iter()
            .any(|issue| issue.field == "envelope.operational_facts")
    );
}

#[test]
fn identical_contract_produces_identical_versioned_cards_and_bytes() {
    let input = facts(EngineeringMode::Mvp);
    let first = compose(input.clone());
    let second = compose(input);
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert!(
        first
            .mandatory_cards
            .iter()
            .all(|card| card.catalogue_version == first.catalogue_version
                && card.id.ends_with("@0.1"))
    );
}

#[test]
fn invalid_contract_shape_is_rejected_before_composition() {
    let mut input = facts(EngineeringMode::Production);
    input.affected_guarantees = known(vec![
        ProtectedGuarantee::Payment,
        ProtectedGuarantee::Payment,
    ]);
    assert_eq!(
        VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
            "task-42".into(),
            "revision-7".into(),
            input
        )
        .unwrap_err(),
        Error::InvalidArguments
    );
    assert_eq!(
        VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
            " ".into(),
            "revision-7".into(),
            facts(EngineeringMode::Demo)
        )
        .unwrap_err(),
        Error::InvalidArguments
    );
}
