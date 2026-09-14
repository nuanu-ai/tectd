use super::*;

fn entry(sequence: i64, unit: u128) -> KnowledgeSuppressionEntry {
    KnowledgeSuppressionEntry {
        tenant_id: Uuid::from_u128(1),
        workspace_id: Uuid::from_u128(2),
        unit_id: Uuid::from_u128(unit),
        change_id: Uuid::from_u128(10 + unit),
        run_id: Uuid::from_u128(20 + unit),
        request_id: Uuid::from_u128(30 + unit),
        event_id: Uuid::from_u128(40 + unit),
        erasure_sequence: sequence,
    }
}

fn valid_manifest() -> KnowledgeSuppressionManifest {
    make_manifest(Uuid::from_u128(9), 2, vec![entry(1, 101), entry(2, 102)]).unwrap()
}

#[test]
fn manifest_bytes_are_deterministic_and_roundtrip() {
    let manifest = valid_manifest();
    let first = knowledge_suppression_manifest_bytes(&manifest).unwrap();
    let second = knowledge_suppression_manifest_bytes(&manifest).unwrap();
    assert_eq!(first, second);
    assert_eq!(parse_knowledge_suppression_manifest(&first), Ok(manifest));
}

#[test]
fn manifest_rejects_gap_reorder_duplicate_unknown_and_truncation() {
    let manifest = valid_manifest();
    let mut gap = manifest.clone();
    gap.entries[1].erasure_sequence = 3;
    assert_eq!(gap.validate(), Err(Error::InvalidArguments));
    let mut reordered = manifest.clone();
    reordered.entries.swap(0, 1);
    assert_eq!(reordered.validate(), Err(Error::InvalidArguments));
    let mut duplicate = manifest.clone();
    duplicate.entries[1].unit_id = duplicate.entries[0].unit_id;
    assert_eq!(duplicate.validate(), Err(Error::InvalidArguments));
    let mut value = serde_json::to_value(&manifest).unwrap();
    value["unknown"] = serde_json::json!(true);
    assert_eq!(
        parse_knowledge_suppression_manifest(&serde_json::to_vec(&value).unwrap()),
        Err(Error::InvalidArguments)
    );
    let bytes = knowledge_suppression_manifest_bytes(&manifest).unwrap();
    assert_eq!(
        parse_knowledge_suppression_manifest(&bytes[..bytes.len() / 2]),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn manifest_cannot_nominate_a_shorter_checkpoint() {
    let mut manifest = valid_manifest();
    manifest.high_water_erasure_sequence = 1;
    assert_eq!(manifest.validate(), Err(Error::InvalidArguments));
}
