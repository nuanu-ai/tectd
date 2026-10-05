use super::*;

fn request() -> PipelineRunMigrationRequest {
    PipelineRunMigrationRequest {
        request_id: Uuid::new_v4(),
        predecessor_run_id: Uuid::new_v4(),
        predecessor_definition_version: "0.6".into(),
        predecessor_definition_digest: "legacy-digest".into(),
        successor_definition_version: "0.7".into(),
        successor_definition_digest: "successor-digest".into(),
        mappings: vec![PipelineObligationMapping {
            legacy_obligation_id: "phase-01".into(),
            successor_obligation_id: "checkpoint-01".into(),
            evidence_refs: vec![PipelineMigrationEvidenceRef {
                reference: "artifact://evidence/1".into(),
                digest: "evidence-digest".into(),
            }],
        }],
    }
}

#[test]
fn explicit_mapping_is_accepted() {
    let migration = request();
    let predecessor = (
        migration.predecessor_run_id,
        migration.predecessor_definition_version.clone(),
        migration.predecessor_definition_digest.clone(),
    );
    assert!(migration.validate().is_ok());
    // Validation is additive metadata only: it cannot rewrite the legacy
    // run identity or its pinned v0.6 definition.
    assert_eq!(
        predecessor,
        (
            migration.predecessor_run_id,
            migration.predecessor_definition_version,
            migration.predecessor_definition_digest
        )
    );
}

#[test]
fn missing_or_ambiguous_mapping_is_refused() {
    let mut missing = request();
    missing.mappings.clear();
    let error = missing.validate().unwrap_err();
    assert_eq!(error.code(), "LEGACY_MIGRATION_REQUIRED");

    let mut ambiguous = request();
    ambiguous.mappings.push(ambiguous.mappings[0].clone());
    let error = ambiguous.validate().unwrap_err();
    assert_eq!(error.code(), "LEGACY_MIGRATION_REQUIRED");
}

#[test]
fn predecessor_definition_cannot_be_reinterpreted_as_successor() {
    let mut request = request();
    request.successor_definition_version = request.predecessor_definition_version.clone();
    assert_eq!(
        request.validate().unwrap_err().code(),
        "LEGACY_MIGRATION_REQUIRED"
    );
}
#[test]
fn migration_retirement_retag_preserves_wrappers_and_unrelated_diagnostics() {
    let original = lightweight_retirement_error(
        "arguments.params.definition_version",
        "retired",
        "begin_current_lightweight_k1k5",
    );
    let refusal = original.refusal().unwrap().clone();
    let wrapped = Error::PipelineRefused {
        source: Box::new(Error::InputConflict),
        refusal: Box::new(refusal.clone()),
    };
    let mut expected_refusal = refusal;
    expected_refusal.path = Some("arguments.params.successor_definition_version".into());
    expected_refusal.next_action =
        Some("get_current_context_and_use_exact_migration_action".into());
    assert_eq!(
        migration_successor_retirement_error(wrapped),
        Error::PipelineRefused {
            source: Box::new(Error::InputConflict),
            refusal: Box::new(expected_refusal)
        }
    );
    let unrelated = Error::refused_at(
        RefusalCode::InvalidOutput,
        "UNRELATED",
        "field",
        "expected",
        "actual",
        "correct",
        "proof",
    );
    assert_eq!(
        migration_successor_retirement_error(unrelated.clone()),
        unrelated
    );
    assert_eq!(
        migration_successor_retirement_error(Error::InvalidArguments),
        Error::InvalidArguments
    );
}

/// A byte-bound port fixture, not an implementation of SHA256.
struct ExpectedBytesPort {
    expected: Vec<u8>,
    answer: [u8; 32],
    calls: std::cell::Cell<usize>,
}
impl PipelineDefinitionDigestPort for ExpectedBytesPort {
    fn sha256(&self, canonical_json: &[u8]) -> [u8; 32] {
        self.calls.set(self.calls.get() + 1);
        if canonical_json == self.expected {
            self.answer
        } else {
            [0; 32]
        }
    }
}
fn canonical_fixture() -> PipelineDefinitionSnapshot {
    serde_json::from_str(include_str!(
        "../../../host/pipeline-definitions/lightweight-tdd-0.7.1-native.k1k5.json"
    ))
    .unwrap()
}
fn fixture_port(definition: &PipelineDefinitionSnapshot) -> ExpectedBytesPort {
    let mut material = definition.clone();
    material.digest.clear();
    let mut answer = [0; 32];
    for (index, byte) in answer.iter_mut().enumerate() {
        *byte = u8::from_str_radix(
            &CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST[index * 2..index * 2 + 2],
            16,
        )
        .unwrap();
    }
    ExpectedBytesPort {
        expected: serde_json::to_vec(&material).unwrap(),
        answer,
        calls: std::cell::Cell::new(0),
    }
}
fn retirement_request(
    predecessor: &PipelineDefinitionSnapshot,
    successor: &PipelineDefinitionSnapshot,
) -> PipelineRunMigrationRequest {
    PipelineRunMigrationRequest {
        predecessor_definition_version: predecessor.version.clone(),
        predecessor_definition_digest: predecessor.digest.clone(),
        successor_definition_version: successor.version.clone(),
        successor_definition_digest: successor.digest.clone(),
        mappings: Vec::new(),
        ..request()
    }
}
#[test]
fn digest_port_receives_typed_order_bytes_with_only_top_digest_cleared() {
    let definition = canonical_fixture();
    let port = fixture_port(&definition);
    assert!(port.expected.starts_with(b"{\"kind\":\"slice.lightweight-tdd-development\",\"version\":\"0.7.1-native.k1k5\",\"digest\":\"\",\"overview\":"));
    let material: serde_json::Value = serde_json::from_slice(&port.expected).unwrap();
    assert_eq!(material["digest"], "");
    assert_eq!(material["overview"]["digest"], definition.overview.digest);
    assert_eq!(
        material["phases"][0]["instructions"][0]["digest"],
        definition.phases[0].instructions[0].digest
    );
    assert_eq!(
        pipeline_definition_digest(&definition, &port).unwrap(),
        CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST
    );
    assert_eq!(port.calls.get(), 1);
    assert_eq!(definition.digest, CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST);
}
#[test]
fn canonical_successor_requires_recomputed_bytes_through_mandatory_port() {
    let definition = canonical_fixture();
    let port = fixture_port(&definition);
    assert!(is_current_lightweight_retirement_successor(
        &definition,
        &port
    ));
    assert_eq!(port.calls.get(), 1);
    let mut changed = definition.clone();
    changed.phases[0].output_contract.push_str("altered");
    assert!(!is_current_lightweight_retirement_successor(
        &changed, &port
    ));
    changed = definition.clone();
    changed.phases[0].id = "arbitrary".into();
    assert!(!is_current_lightweight_retirement_successor(
        &changed, &port
    ));
    changed = definition.clone();
    changed.digest = "false metadata".into();
    assert!(!is_current_lightweight_retirement_successor(
        &changed, &port
    ));
}
#[test]
fn retirement_empty_mapping_exception_still_requires_actual_metadata_and_digest() {
    let successor = canonical_fixture();
    let port = fixture_port(&successor);
    let mut predecessor = successor.clone();
    predecessor.version = "0.6.0-native.engineering.2".into();
    predecessor.digest = "immutable legacy digest".into();
    let migration = retirement_request(&predecessor, &successor);
    assert!(
        migration
            .validate_retirement_restart(&predecessor, &successor, &port)
            .is_ok()
    );
    assert!(migration.validate().is_err());
    let mut false_metadata = migration.clone();
    false_metadata.successor_definition_digest = "false metadata".into();
    assert!(
        false_metadata
            .validate_retirement_restart(&predecessor, &successor, &port)
            .is_err()
    );
    let mut altered = successor.clone();
    altered.phases[0].output_contract.push_str("altered");
    assert!(
        migration
            .validate_retirement_restart(&predecessor, &altered, &port)
            .is_err()
    );
    assert!(
        migration
            .validate_retirement_restart(&successor, &successor, &port)
            .is_err()
    );
    assert!(
        request()
            .validate_retirement_restart(&predecessor, &altered, &port)
            .is_ok()
    );
}
