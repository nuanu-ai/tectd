use super::*;
use crate::{
    CommitmentEvidence, EngineeringIntent, EngineeringMode, FactProvenance, OperatingEnvelope,
    OperatingFact, ProtectedGuarantee,
};

fn source() -> FactProvenance {
    FactProvenance("task:42@7".into())
}

fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: source(),
    }
}

fn input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: known(EngineeringMode::Mvp),
        envelope: OperatingEnvelope {
            scale: known("12 workers".into()),
            operational_facts: OperationalFacts::Reported {
                entries: vec![
                    OperatingFact {
                        name: "region/zone~1".into(),
                        fact: known("bali".into()),
                    },
                    OperatingFact {
                        name: "users".into(),
                        fact: MatrixFact::KnownEmpty {
                            provenance: source(),
                        },
                    },
                ],
            },
        },
        criticality: known("low".into()),
        intent: known(EngineeringIntent::Other("booking".into())),
        urgency: known("normal".into()),
        promised_behavior: known("books".into()),
        promised_proof: known("acceptance".into()),
        affected_guarantees: MatrixFact::KnownEmpty {
            provenance: source(),
        },
        actual_exposure: known(false),
        demand_commitment: known(CommitmentEvidence::NoCommitment),
        latency_commitment: known(CommitmentEvidence::NoCommitment),
        urgent_repair: known(false),
    }
}

fn record(input: &EngineeringMatrixInput) -> MatrixVerificationRecord {
    let bindings = required_matrix_facts(input)
        .unwrap()
        .into_iter()
        .map(|fact| MatrixEvidenceBinding {
            fact_path: fact.path,
            value_digest: fact.value_digest,
            evidence_ref: "immutable:artifact@v1".into(),
            content_digest: "a".repeat(64),
            source: "source-system".into(),
            subject: "task:42".into(),
            observed_at: 10,
            expires_at: 30,
            validation_outcome: EvidenceValidationOutcome::Accepted,
        })
        .collect();
    let mut record = MatrixVerificationRecord {
        schema: MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: "task-42".into(),
        task_revision: "7".into(),
        input_digest: matrix_input_digest(input).unwrap(),
        owner_principal: "owner".into(),
        verifier_principal: "reviewer".into(),
        policy_version: "source-check/1".into(),
        bindings,
        digest: String::new(),
    };
    record.digest = record.canonical_digest().unwrap();
    record
}

fn reseal(record: &mut MatrixVerificationRecord) {
    record.digest = record.canonical_digest().unwrap();
}

fn evaluate(
    input: &EngineeringMatrixInput,
    record: &MatrixVerificationRecord,
    now: i64,
) -> Result<ValidatedMatrixVerification> {
    evaluate_matrix_verification("task-42", "7", input, record, now)
}

#[test]
fn complete_coverage_includes_dynamic_and_known_empty() {
    let input = input();
    let facts = required_matrix_facts(&input).unwrap();
    assert_eq!(facts.len(), 14);
    assert!(
        facts
            .iter()
            .any(|fact| fact.path == "/envelope/operational_facts/region~1zone~01")
    );
    assert!(
        facts
            .iter()
            .any(|fact| fact.path == "/envelope/operational_facts/users")
    );
    let record = record(&input);
    assert_eq!(
        evaluate(&input, &record, 20).unwrap().record_digest,
        record.digest
    );
    let mut reordered = input.clone();
    if let OperationalFacts::Reported { entries } = &mut reordered.envelope.operational_facts {
        entries.reverse();
    }
    assert_ne!(
        matrix_input_digest(&input).unwrap(),
        matrix_input_digest(&reordered).unwrap()
    );
}

#[test]
fn input_digest_matches_persisted_matrix_task_store_json_hash() {
    let input = input();
    // This is the same projection and hash as
    // application::canonical_matrix_input_digest, used by PgUnitOfWork.
    let persisted = serde_json::to_value(&input).unwrap();
    let expected = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&persisted).unwrap())
    );
    assert_eq!(
        expected,
        "23a00caa4c8e6212d660e388c3d4791631dca2936ba3bc5b540f97407354ff56"
    );
    assert_eq!(matrix_input_digest(&input).unwrap(), expected);
    assert_ne!(
        matrix_input_digest(&input).unwrap(),
        digest_json(&(MATRIX_VERIFICATION_SCHEMA, input)).unwrap()
    );
}

#[test]
fn missing_duplicate_and_wrong_value_fail() {
    let input = input();
    let mut value = record(&input);
    value.bindings.pop();
    reseal(&mut value);
    assert!(evaluate(&input, &value, 20).is_err());
    let mut value = record(&input);
    value.bindings[0].value_digest = "b".repeat(64);
    reseal(&mut value);
    assert!(evaluate(&input, &value, 20).is_err());
    let mut value = record(&input);
    value.bindings[1] = value.bindings[0].clone();
    reseal(&mut value);
    assert!(evaluate(&input, &value, 20).is_err());
}

#[test]
fn unresolved_states_and_invalid_input_fail_closed() {
    let original = input();
    for fact in [
        MatrixFact::Unknown {
            provenance: source(),
        },
        MatrixFact::Gap {
            provenance: source(),
        },
        MatrixFact::Conflict {
            provenance: source(),
        },
        MatrixFact::Invalid {
            provenance: source(),
        },
        MatrixFact::Absent,
    ] {
        let mut input = original.clone();
        input.criticality = fact;
        assert!(required_matrix_facts(&input).is_err());
    }
    let mut invalid = original;
    invalid.envelope.operational_facts = OperationalFacts::Reported { entries: vec![] };
    assert!(required_matrix_facts(&invalid).is_err());
}

#[test]
fn stale_rejected_and_tampered_bindings_fail() {
    let input = input();
    let mut value = record(&input);
    value.bindings[0].expires_at = 20;
    reseal(&mut value);
    assert!(evaluate(&input, &value, 20).is_err());
    let mut value = record(&input);
    value.bindings[0].validation_outcome = EvidenceValidationOutcome::Rejected;
    reseal(&mut value);
    assert!(evaluate(&input, &value, 20).is_err());
    let mut value = record(&input);
    value.owner_principal = value.verifier_principal.clone();
    reseal(&mut value);
    assert!(evaluate(&input, &value, 20).is_err());
    let mut value = record(&input);
    value.bindings[0].source = "tampered".into();
    assert!(evaluate(&input, &value, 20).is_err());
    let value = record(&input);
    assert!(evaluate_matrix_verification("other-task", "7", &input, &value, 20).is_err());
}

#[test]
fn known_guarantees_order_affects_persisted_input_digest() {
    let mut input = input();
    input.affected_guarantees = known(vec![
        ProtectedGuarantee::Secret,
        ProtectedGuarantee::Payment,
    ]);
    let original = matrix_input_digest(&input).unwrap();
    let facts = required_matrix_facts(&input).unwrap();
    if let MatrixFact::Known { value, .. } = &mut input.affected_guarantees {
        value.reverse();
    }
    assert_ne!(matrix_input_digest(&input).unwrap(), original);
    assert_eq!(required_matrix_facts(&input).unwrap(), facts);
}
