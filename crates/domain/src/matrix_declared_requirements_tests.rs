use super::*;
use crate::{OperatingEnvelope, OperationalFacts};

fn recorder() -> DeclarationRecorder {
    DeclarationRecorder {
        principal: "agent".into(),
        session: "session".into(),
    }
}
fn program() -> RequirementsAnchor {
    RequirementsAnchor::Program {
        program_id: Uuid::from_u128(1),
    }
}
fn scope() -> RequirementsAnchor {
    RequirementsAnchor::Scope {
        program_id: Uuid::from_u128(1),
        scope_id: Uuid::from_u128(2),
    }
}
fn slice(id: u128) -> RequirementsAnchor {
    RequirementsAnchor::Slice {
        program_id: Uuid::from_u128(1),
        scope_id: Uuid::from_u128(2),
        candidate_set_id: Uuid::from_u128(20),
        work_candidate_id: Uuid::from_u128(id),
    }
}
fn set(value: DeclaredRequirementValue) -> RequirementDeclarationPatch {
    RequirementDeclarationPatch::Set { value }
}
fn revision(
    anchor: RequirementsAnchor,
    number: u64,
    patches: Vec<RequirementDeclarationPatch>,
    confirmed: bool,
) -> MatrixRequirementsRevision {
    let proposal = MatrixRequirementsProposal::new(anchor, number, patches, recorder()).unwrap();
    let confirmation = confirmed.then(|| {
        MatrixRequirementsConfirmation::new(
            &proposal,
            proposal.revision(),
            proposal.digest().into(),
            "owner".into(),
            "explicit-owner-response".into(),
            recorder(),
        )
        .unwrap()
    });
    MatrixRequirementsRevision {
        proposal,
        confirmation,
    }
}
fn declarations() -> Vec<RequirementDeclarationPatch> {
    vec![
        set(DeclaredRequirementValue::Mode(EngineeringMode::Mvp)),
        set(DeclaredRequirementValue::Intent(EngineeringIntent::Other(
            "build".into(),
        ))),
        set(DeclaredRequirementValue::Urgency("normal".into())),
        set(DeclaredRequirementValue::PromisedBehavior("booking".into())),
        set(DeclaredRequirementValue::PromisedProof("regression".into())),
        set(DeclaredRequirementValue::NoDemandCommitment),
        set(DeclaredRequirementValue::NoLatencyCommitment),
    ]
}
fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("observed-source".into()),
    }
}
fn input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: known("small observed workload".into()),
            operational_facts: OperationalFacts::KnownEmpty {
                provenance: FactProvenance("observed-source".into()),
            },
        },
        criticality: known("limited".into()),
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        affected_guarantees: MatrixFact::KnownEmpty {
            provenance: FactProvenance("observed-source".into()),
        },
        actual_exposure: known(false),
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: known(false),
    }
}

#[test]
fn positive_commitment_is_a_digest_bound_promise_not_a_measured_status() {
    let mut patches = declarations();
    patches[5] = set(DeclaredRequirementValue::DemandCommitment(
        "100 requests per second".into(),
    ));
    patches[6] = set(DeclaredRequirementValue::LatencyCommitment(
        "p95 under 200 ms".into(),
    ));
    let first = MatrixRequirementsProposal::new(program(), 1, patches.clone(), recorder()).unwrap();
    let replay =
        MatrixRequirementsProposal::new(program(), 1, patches.clone(), recorder()).unwrap();
    assert_eq!(first.digest(), replay.digest());
    let mut changed = patches.clone();
    changed[5] = set(DeclaredRequirementValue::DemandCommitment(
        "200 requests per second".into(),
    ));
    assert_ne!(
        first.digest(),
        MatrixRequirementsProposal::new(program(), 1, changed, recorder())
            .unwrap()
            .digest()
    );
    let confirmed = revision(program(), 1, patches, true);
    let context = resolve_matrix_requirements(
        &[program()],
        std::slice::from_ref(&confirmed),
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    let context_replay =
        resolve_matrix_requirements(&[program()], &[confirmed], MATRIX_REQUIREMENTS_SCHEMA)
            .unwrap();
    assert_eq!(context.semantic_digest(), context_replay.semantic_digest());
    let mut operating = input();
    operating.demand_commitment = known(CommitmentEvidence::ExceedsVerifiedLimit);
    operating.latency_commitment = known(CommitmentEvidence::WithinVerifiedLimit);
    let required = required_matrix_operating_facts(&context, &operating).unwrap();
    assert!(
        required
            .iter()
            .any(|fact| fact.path == "/demand_commitment")
    );
    assert!(
        required
            .iter()
            .any(|fact| fact.path == "/latency_commitment")
    );
    let composition =
        compose_declared_requirements_matrix(&context, "task".into(), "1".into(), &operating)
            .unwrap();
    assert!(
        composition
            .mandatory_cards
            .iter()
            .any(|card| card.id == "EM02-CAPACITY@0.1")
    );
    assert!(
        !composition.is_resolved(),
        "a promise and caller claim are not verification"
    );

    operating.demand_commitment = known(CommitmentEvidence::NoCommitment);
    assert!(bind_matrix_requirements_input(&context, &operating).is_err());
    operating.demand_commitment = known(CommitmentEvidence::LacksEvidence);
    assert!(required_matrix_operating_facts(&context, &operating).is_err());

    let mut without_promise = declarations();
    without_promise.remove(5);
    let missing = resolve_matrix_requirements(
        &[program()],
        &[revision(program(), 1, without_promise, true)],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    operating.demand_commitment = known(CommitmentEvidence::ExceedsVerifiedLimit);
    assert!(bind_matrix_requirements_input(&missing, &operating).is_err());
    let old = resolve_matrix_requirements(
        &[program()],
        &[revision(program(), 1, declarations(), true)],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert!(bind_matrix_requirements_input(&old, &operating).is_err());
    assert!(matches!(
        bind_matrix_requirements_input(&old, &input())
            .unwrap()
            .demand_commitment,
        MatrixFact::Known {
            value: CommitmentEvidence::NoCommitment,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(DeclaredRequirementValue::NoDemandCommitment).unwrap(),
        serde_json::json!({"kind":"no_demand_commitment"})
    );
}

#[test]
fn positive_commitment_statement_requires_canonical_bounded_text() {
    for statement in [
        "",
        " ",
        " leading",
        "trailing ",
        "line\nbreak",
        &"x".repeat(257),
    ] {
        assert!(
            MatrixRequirementsProposal::new(
                program(),
                1,
                vec![set(DeclaredRequirementValue::DemandCommitment(
                    statement.into()
                ))],
                recorder(),
            )
            .is_err()
        );
    }
    let statement = "x".repeat(256);
    assert!(
        MatrixRequirementsProposal::new(
            program(),
            1,
            vec![set(DeclaredRequirementValue::LatencyCommitment(statement))],
            recorder(),
        )
        .is_ok()
    );
}
#[test]
fn exact_confirmation_binding_rejects_wrong_revision_digest_and_empty_response() {
    let p = MatrixRequirementsProposal::new(program(), 1, declarations(), recorder()).unwrap();
    for (revision, digest, reference) in [
        (2, p.digest().into(), "response"),
        (1, "wrong".into(), "response"),
        (1, p.digest().into(), ""),
    ] {
        assert!(
            MatrixRequirementsConfirmation::new(
                &p,
                revision,
                digest,
                "owner".into(),
                reference.into(),
                recorder()
            )
            .is_err()
        );
    }
}
#[test]
fn inheritance_pending_override_and_slice_isolation() {
    let parent = revision(program(), 1, declarations(), true);
    let pending = revision(
        scope(),
        1,
        vec![set(DeclaredRequirementValue::Mode(
            EngineeringMode::Production,
        ))],
        false,
    );
    let override_ = revision(
        slice(3),
        1,
        vec![set(DeclaredRequirementValue::Mode(EngineeringMode::Demo))],
        true,
    );
    let inherited = resolve_matrix_requirements(
        &[program(), scope()],
        &[parent.clone(), pending.clone()],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert_eq!(
        inherited.values()[&DeclaredRequirementPath::Mode].value,
        DeclaredRequirementValue::Mode(EngineeringMode::Mvp)
    );
    let overridden = resolve_matrix_requirements(
        &[program(), scope(), slice(3)],
        &[parent.clone(), pending, override_],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert_eq!(
        overridden.values()[&DeclaredRequirementPath::Mode].value,
        DeclaredRequirementValue::Mode(EngineeringMode::Demo)
    );
    let sibling = resolve_matrix_requirements(
        &[program(), scope(), slice(4)],
        &[parent],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert_eq!(sibling.semantic_digest(), inherited.semantic_digest());
}
#[test]
fn removal_missing_and_hidden_parent_edit_semantic_digest_stability() {
    let parent = revision(program(), 1, declarations(), true);
    let child = revision(
        scope(),
        1,
        vec![set(DeclaredRequirementValue::Mode(EngineeringMode::Demo))],
        true,
    );
    let a = resolve_matrix_requirements(
        &[program(), scope()],
        &[parent.clone(), child.clone()],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    let edit = revision(
        program(),
        2,
        vec![set(DeclaredRequirementValue::Mode(
            EngineeringMode::Production,
        ))],
        true,
    );
    let b = resolve_matrix_requirements(
        &[program(), scope()],
        &[parent.clone(), edit, child.clone()],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert_eq!(a.semantic_digest(), b.semantic_digest());
    let changed = resolve_matrix_requirements(
        &[program()],
        std::slice::from_ref(&parent),
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert_ne!(a.semantic_digest(), changed.semantic_digest());
    let schema_changed = resolve_matrix_requirements(
        &[program(), scope()],
        &[parent.clone(), child],
        "tect.matrix-requirements/2",
    )
    .unwrap();
    assert_ne!(a.semantic_digest(), schema_changed.semantic_digest());
    let remove = revision(
        scope(),
        2,
        vec![RequirementDeclarationPatch::Remove {
            path: DeclaredRequirementPath::Mode,
        }],
        true,
    );
    let missing = resolve_matrix_requirements(
        &[program(), scope()],
        &[parent, remove],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert_eq!(
        missing_required_matrix_declarations(&missing),
        vec![DeclaredRequirementPath::Mode]
    );
}
#[test]
fn static_roles_and_partition_do_not_promote_legacy_claims() {
    let context = resolve_matrix_requirements(
        &[program()],
        &[revision(program(), 1, declarations(), true)],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    let mut value = input();
    assert_eq!(
        matrix_fact_role("/mode", &value).unwrap(),
        MatrixFactRole::DeclaredRequirement
    );
    assert_eq!(
        matrix_fact_role("/actual_exposure", &value).unwrap(),
        MatrixFactRole::OperatingEvidence
    );
    value.demand_commitment = known(CommitmentEvidence::LacksEvidence);
    assert_eq!(
        matrix_fact_role("/demand_commitment", &value).unwrap(),
        MatrixFactRole::Unresolved
    );
    value.demand_commitment = MatrixFact::Absent;
    let operating = required_matrix_operating_facts(&context, &value).unwrap();
    assert!(
        !operating
            .iter()
            .any(|f| f.path == "/mode" || f.path == "/promised_proof")
    );
    assert!(operating.iter().any(|f| f.path == "/actual_exposure"));
    let composition =
        compose_declared_requirements_matrix(&context, "task".into(), "1".into(), &value).unwrap();
    assert!(!composition.is_resolved());
    let legacy = bind_matrix_requirements_input(&context, &value).unwrap();
    let empty = resolve_matrix_requirements(&[program()], &[], MATRIX_REQUIREMENTS_SCHEMA).unwrap();
    assert!(bind_matrix_requirements_input(&empty, &legacy).is_err());
}
#[test]
fn context_input_conflicts_unknowns_and_cross_program_denied() {
    let parent = revision(program(), 1, declarations(), true);
    let context = resolve_matrix_requirements(
        &[program()],
        std::slice::from_ref(&parent),
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    let mut value = input();
    value.mode = known(EngineeringMode::Production);
    assert!(bind_matrix_requirements_input(&context, &value).is_err());
    value.mode = MatrixFact::Unknown {
        provenance: FactProvenance("unknown".into()),
    };
    assert!(bind_matrix_requirements_input(&context, &value).is_err());
    let foreign = RequirementsAnchor::Scope {
        program_id: Uuid::from_u128(99),
        scope_id: Uuid::from_u128(2),
    };
    assert!(
        resolve_matrix_requirements(
            &[program(), foreign],
            std::slice::from_ref(&parent),
            MATRIX_REQUIREMENTS_SCHEMA
        )
        .is_err()
    );
    assert!(
        resolve_matrix_requirements(
            &[program(), scope()],
            &[parent.clone(), parent],
            MATRIX_REQUIREMENTS_SCHEMA
        )
        .is_err()
    );
}

#[test]
fn hydrated_records_are_revalidated_and_promises_are_not_evidence() {
    let mut entry = revision(program(), 1, declarations(), true);
    let mut wire = serde_json::to_value(&entry.proposal).unwrap();
    wire["revision"] = serde_json::json!(2);
    entry.proposal = serde_json::from_value(wire).unwrap();
    assert!(
        resolve_matrix_requirements(&[program()], &[entry], MATRIX_REQUIREMENTS_SCHEMA).is_err()
    );
    let mut entry = revision(program(), 1, declarations(), true);
    let mut wire = serde_json::to_value(entry.confirmation.as_ref().unwrap()).unwrap();
    wire["owner_response_ref"] = serde_json::json!("");
    entry.confirmation = Some(serde_json::from_value(wire).unwrap());
    assert!(
        resolve_matrix_requirements(&[program()], &[entry], MATRIX_REQUIREMENTS_SCHEMA).is_err()
    );
    let context = resolve_matrix_requirements(
        &[program()],
        &[revision(program(), 1, declarations(), true)],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    let mut operating = input();
    operating.actual_exposure = MatrixFact::Absent;
    assert!(required_matrix_operating_facts(&context, &operating).is_err());
    operating.actual_exposure = known(false);
    operating.demand_commitment = known(CommitmentEvidence::LacksEvidence);
    assert!(required_matrix_operating_facts(&context, &operating).is_err());
}

#[test]
fn logical_slice_anchor_keeps_scope_and_work_identity() {
    let leaf = slice(3);
    let wire = serde_json::to_value(leaf).unwrap();
    assert!(wire.get("slice_id").is_none());
    assert_eq!(
        wire["candidate_set_id"],
        serde_json::json!(Uuid::from_u128(20))
    );
    assert_eq!(
        serde_json::from_value::<RequirementsAnchor>(wire).unwrap(),
        leaf
    );
    let wrong_scope = RequirementsAnchor::Slice {
        program_id: Uuid::from_u128(1),
        scope_id: Uuid::from_u128(99),
        candidate_set_id: Uuid::from_u128(20),
        work_candidate_id: Uuid::from_u128(3),
    };
    assert!(
        resolve_matrix_requirements(
            &[program(), scope(), wrong_scope],
            &[],
            MATRIX_REQUIREMENTS_SCHEMA
        )
        .is_err()
    );
}
