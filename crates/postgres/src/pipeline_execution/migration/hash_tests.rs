use super::*;

fn current_definition() -> PipelineDefinitionSnapshot {
    serde_json::from_str(include_str!(
        "../../../../host/pipeline-definitions/lightweight-tdd-0.7.1-native.k1k5.json"
    ))
    .expect("current immutable definition fixture")
}

#[test]
fn local_adapter_hashes_genuine_canonical_successor_and_rejects_forged_body() {
    let definition = current_definition();
    let actual = tect_domain::pipeline_definition_digest(&definition, &MigrationDefinitionDigest)
        .expect("canonical typed serialization");
    assert_eq!(actual, tect_domain::CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST);
    assert!(tect_domain::is_current_lightweight_retirement_successor(
        &definition,
        &MigrationDefinitionDigest
    ));
    let mut forged = definition.clone();
    forged.overview.body.push_str(" forged body");
    assert_eq!(forged.digest, definition.digest);
    let changed = tect_domain::pipeline_definition_digest(&forged, &MigrationDefinitionDigest)
        .expect("canonical typed serialization");
    assert_ne!(changed, actual);
    assert!(!tect_domain::is_current_lightweight_retirement_successor(
        &forged,
        &MigrationDefinitionDigest
    ));
}

#[test]
fn local_adapter_clears_only_top_level_declared_digest() {
    let definition = current_definition();
    let mut altered_pin = definition.clone();
    altered_pin.digest = "untrusted declared pin".into();
    assert_eq!(
        tect_domain::pipeline_definition_digest(&definition, &MigrationDefinitionDigest).unwrap(),
        tect_domain::pipeline_definition_digest(&altered_pin, &MigrationDefinitionDigest).unwrap()
    );
    let mut altered_nested_digest = definition.clone();
    altered_nested_digest.overview.digest = "untrusted nested pin".into();
    assert_ne!(
        tect_domain::pipeline_definition_digest(&definition, &MigrationDefinitionDigest).unwrap(),
        tect_domain::pipeline_definition_digest(&altered_nested_digest, &MigrationDefinitionDigest)
            .unwrap()
    );
}

#[test]
fn local_adapter_gates_empty_retirement_restart_mapping_on_the_actual_successor_body() {
    let successor = current_definition();
    let mut predecessor = successor.clone();
    predecessor.version = "0.4.0-native.skills.1".into();
    predecessor.digest = "legacy-stored-pin".into();
    let request = PipelineRunMigrationRequest {
        request_id: Uuid::new_v4(),
        predecessor_run_id: Uuid::new_v4(),
        predecessor_definition_version: predecessor.version.clone(),
        predecessor_definition_digest: predecessor.digest.clone(),
        successor_definition_version: successor.version.clone(),
        successor_definition_digest: successor.digest.clone(),
        mappings: vec![],
    };
    assert!(
        request
            .validate_retirement_restart(&predecessor, &successor, &MigrationDefinitionDigest)
            .is_ok()
    );
    let mut forged = successor.clone();
    forged.overview.body.push_str(" forged body");
    assert!(
        request
            .validate_retirement_restart(&predecessor, &forged, &MigrationDefinitionDigest)
            .is_err()
    );
}
