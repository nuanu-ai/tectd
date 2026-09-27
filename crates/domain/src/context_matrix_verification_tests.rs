use super::*;
use crate::*;
use uuid::Uuid;

const SNAPSHOT_ID: &str = "00000000-0000-0000-0000-000000000002";
const NEXT_SNAPSHOT_ID: &str = "00000000-0000-0000-0000-000000000003";

fn recorder() -> DeclarationRecorder {
    DeclarationRecorder {
        principal: "agent".into(),
        session: "session".into(),
    }
}
fn anchor() -> RequirementsAnchor {
    RequirementsAnchor::Program {
        program_id: Uuid::from_u128(1),
    }
}
fn revision(number: u64, mode: EngineeringMode, confirmed: bool) -> MatrixRequirementsRevision {
    let values = vec![
        DeclaredRequirementValue::Mode(mode),
        DeclaredRequirementValue::Intent(EngineeringIntent::Other("build".into())),
        DeclaredRequirementValue::Urgency("normal".into()),
        DeclaredRequirementValue::PromisedBehavior("booking".into()),
        DeclaredRequirementValue::PromisedProof("regression".into()),
        DeclaredRequirementValue::NoDemandCommitment,
        DeclaredRequirementValue::NoLatencyCommitment,
    ];
    let proposal = MatrixRequirementsProposal::new(
        anchor(),
        number,
        values
            .into_iter()
            .map(|value| RequirementDeclarationPatch::Set { value })
            .collect(),
        recorder(),
    )
    .unwrap();
    let confirmation = confirmed.then(|| {
        MatrixRequirementsConfirmation::new(
            &proposal,
            number,
            proposal.digest().into(),
            "owner".into(),
            "human-response".into(),
            recorder(),
        )
        .unwrap()
    });
    MatrixRequirementsRevision {
        proposal,
        confirmation,
    }
}
fn context(mode: EngineeringMode) -> EffectiveMatrixRequirements {
    resolve_matrix_requirements(
        &[anchor()],
        &[revision(1, mode, true)],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap()
}
fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("observed".into()),
    }
}
fn input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: known("observed limited workload".into()),
            operational_facts: OperationalFacts::KnownEmpty {
                provenance: FactProvenance("observed".into()),
            },
        },
        criticality: known("limited".into()),
        affected_guarantees: MatrixFact::KnownEmpty {
            provenance: FactProvenance("observed".into()),
        },
        actual_exposure: known(false),
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: known(false),
    }
}
fn record(
    input: &EngineeringMatrixInput,
    context: &EffectiveMatrixRequirements,
) -> ContextMatrixVerificationRecord {
    let bindings = required_matrix_operating_facts(context, input)
        .unwrap()
        .into_iter()
        .map(|fact| MatrixEvidenceBinding {
            fact_path: fact.path,
            value_digest: fact.value_digest,
            evidence_ref: "immutable-ref".into(),
            content_digest: "a".repeat(64),
            source: "source".into(),
            subject: "task".into(),
            observed_at: 10,
            expires_at: 100,
            validation_outcome: EvidenceValidationOutcome::Accepted,
        })
        .collect();
    let mut record = ContextMatrixVerificationRecord {
        schema: CONTEXT_MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: "task".into(),
        task_revision: "1".into(),
        frozen_snapshot_id: SNAPSHOT_ID.into(),
        authority_schema: context.schema().into(),
        input_digest: matrix_input_digest(&bind_matrix_requirements_input(context, input).unwrap())
            .unwrap(),
        requirements_semantic_digest: context.semantic_digest().into(),
        owner_principal: "owner".into(),
        verifier_principal: "verifier".into(),
        policy_version: "policy/1".into(),
        bindings,
        digest: String::new(),
    };
    record.digest = record.canonical_digest().unwrap();
    record
}
#[test]
fn hybrid_exact_subset_and_promised_proof_never_obtained_proof() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let record = record(&input, &context);
    assert!(record.bindings.iter().all(|b| !matches!(
        b.fact_path.as_str(),
        "/mode"
            | "/intent"
            | "/urgency"
            | "/promised_behavior"
            | "/promised_proof"
            | "/demand_commitment"
            | "/latency_commitment"
    )));
    assert!(
        record
            .bindings
            .iter()
            .any(|b| b.fact_path == "/actual_exposure")
    );
    assert!(
        record
            .bindings
            .iter()
            .any(|b| b.fact_path == "/envelope/operational_facts")
    );
    let token = evaluate_context_matrix_verification(
        "task",
        "1",
        SNAPSHOT_ID,
        &input,
        &context,
        &record,
        20,
    )
    .unwrap();
    let composed = compose_confirmed_requirements_matrix(
        "task",
        "1",
        SNAPSHOT_ID,
        &input,
        &context,
        &token,
        20,
    )
    .unwrap();
    assert!(composed.is_resolved());
    assert_eq!(composed.frozen_snapshot_id(), SNAPSHOT_ID);
    assert_eq!(composed.authority_schema(), context.schema());
    assert_eq!(
        composed.requirements_semantic_digest(),
        context.semantic_digest()
    );
    assert_eq!(
        composed.operating_verification_digest(),
        record.digest.as_str()
    );
    assert!(!composed.composition().is_resolved());
    assert_eq!(
        composed.status(),
        ContextMatrixResolutionStatus::ConfirmedRequirementsValidatedOperatingEvidence
    );
    assert!(
        compose_confirmed_requirements_matrix(
            "task",
            "1",
            SNAPSHOT_ID,
            &input,
            &context,
            &token,
            100
        )
        .is_err()
    );

    let different_schema = resolve_matrix_requirements(
        &[anchor()],
        &[revision(1, EngineeringMode::Mvp, true)],
        "tect.matrix-requirements/other",
    )
    .unwrap();
    assert!(
        compose_confirmed_requirements_matrix(
            "task",
            "1",
            SNAPSHOT_ID,
            &input,
            &different_schema,
            &token,
            20,
        )
        .is_err()
    );
}
#[test]
fn missing_rejected_stale_duplicate_and_empty_operating_coverage_denied() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let baseline = record(&input, &context);
    for case in 0..5 {
        let mut record = baseline.clone();
        match case {
            0 => {
                record.bindings.pop();
            }
            1 => record.bindings[0].validation_outcome = EvidenceValidationOutcome::Rejected,
            2 => record.bindings[0].expires_at = 20,
            3 => record.bindings[0] = record.bindings[1].clone(),
            _ => record.bindings.clear(),
        }
        record.digest = record.canonical_digest().unwrap();
        assert!(
            evaluate_context_matrix_verification(
                "task",
                "1",
                SNAPSHOT_ID,
                &input,
                &context,
                &record,
                20
            )
            .is_err()
        );
    }
    let mut missing = input.clone();
    missing.actual_exposure = MatrixFact::Absent;
    assert!(
        evaluate_context_matrix_verification(
            "task",
            "1",
            SNAPSHOT_ID,
            &missing,
            &context,
            &baseline,
            20
        )
        .is_err()
    );
}
#[test]
fn token_rejects_context_and_operating_mutation_legacy_recorder_not_confirmation() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let record = record(&input, &context);
    let token = evaluate_context_matrix_verification(
        "task",
        "1",
        SNAPSHOT_ID,
        &input,
        &context,
        &record,
        20,
    )
    .unwrap();
    let changed = super::tests::context(EngineeringMode::Demo);
    assert!(
        compose_confirmed_requirements_matrix(
            "task",
            "1",
            SNAPSHOT_ID,
            &input,
            &changed,
            &token,
            20
        )
        .is_err()
    );
    let mut exposure = input.clone();
    exposure.actual_exposure = known(true);
    assert!(
        compose_confirmed_requirements_matrix(
            "task",
            "1",
            SNAPSHOT_ID,
            &exposure,
            &context,
            &token,
            20
        )
        .is_err()
    );
    let pending = resolve_matrix_requirements(
        &[anchor()],
        &[revision(1, EngineeringMode::Mvp, false)],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert!(
        evaluate_context_matrix_verification(
            "task",
            "1",
            SNAPSHOT_ID,
            &input,
            &pending,
            &record,
            20
        )
        .is_err()
    );
}
#[test]
fn same_effective_values_different_confirmation_revision_keep_token_valid() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let record = record(&input, &context);
    let token = evaluate_context_matrix_verification(
        "task",
        "1",
        SNAPSHOT_ID,
        &input,
        &context,
        &record,
        20,
    )
    .unwrap();
    let next = resolve_matrix_requirements(
        &[anchor()],
        &[revision(2, EngineeringMode::Mvp, true)],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    assert_eq!(context.semantic_digest(), next.semantic_digest());
    let encoded = serde_json::to_vec(&next).unwrap();
    let decoded: EffectiveMatrixRequirements = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, next);
    assert!(
        compose_confirmed_requirements_matrix("task", "1", SNAPSHOT_ID, &input, &next, &token, 20)
            .unwrap()
            .is_resolved()
    );
    assert!(
        compose_confirmed_requirements_matrix(
            "task",
            "1",
            NEXT_SNAPSHOT_ID,
            &input,
            &next,
            &token,
            20,
        )
        .is_err()
    );
}

#[test]
fn snapshot_and_authority_schema_are_digest_bound_and_current() {
    let input = input();
    let context = context(EngineeringMode::Mvp);
    let baseline = record(&input, &context);

    let token = evaluate_context_matrix_verification(
        "task",
        "1",
        SNAPSHOT_ID,
        &input,
        &context,
        &baseline,
        20,
    )
    .unwrap();
    assert!(
        compose_confirmed_requirements_matrix(
            "task",
            "1",
            NEXT_SNAPSHOT_ID,
            &input,
            &context,
            &token,
            20,
        )
        .is_err()
    );

    let mut wrong_snapshot = baseline.clone();
    wrong_snapshot.frozen_snapshot_id = NEXT_SNAPSHOT_ID.into();
    wrong_snapshot.digest = wrong_snapshot.canonical_digest().unwrap();
    assert!(
        evaluate_context_matrix_verification(
            "task",
            "1",
            SNAPSHOT_ID,
            &input,
            &context,
            &wrong_snapshot,
            20,
        )
        .is_err()
    );

    for invalid in [
        "00000000-0000-0000-0000-000000000000",
        "00000000000000000000000000000002",
        "00000000-0000-0000-0000-000000000002 ",
    ] {
        let mut record = baseline.clone();
        record.frozen_snapshot_id = invalid.into();
        record.digest = record.canonical_digest().unwrap();
        assert!(
            evaluate_context_matrix_verification(
                "task", "1", invalid, &input, &context, &record, 20,
            )
            .is_err()
        );
    }

    let mut wrong_schema = baseline.clone();
    wrong_schema.authority_schema = "tect.matrix-requirements/other".into();
    wrong_schema.digest = wrong_schema.canonical_digest().unwrap();
    assert!(
        evaluate_context_matrix_verification(
            "task",
            "1",
            SNAPSHOT_ID,
            &input,
            &context,
            &wrong_schema,
            20,
        )
        .is_err()
    );

    let mut digest_mutation = baseline.clone();
    digest_mutation.authority_schema = "tect.matrix-requirements/other".into();
    assert_ne!(digest_mutation.canonical_digest().unwrap(), baseline.digest);
    assert!(
        evaluate_context_matrix_verification(
            "task",
            "1",
            SNAPSHOT_ID,
            &input,
            &context,
            &digest_mutation,
            20,
        )
        .is_err()
    );
}
