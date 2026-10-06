use super::*;
use std::sync::Mutex;

struct FixturePort(Mutex<Vec<Vec<u8>>>);
impl crate::PipelineDefinitionDigestPort for FixturePort {
    fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
        self.0.lock().unwrap().push(bytes.to_vec());
        [0xab; 32]
    }
}
fn query(context: &PipelineRunContext) -> PipelineRunContextQuery {
    serde_json::from_value(serde_json::json!({"run_id":context.run.id,"view":"receipt_diff","phase_id":"phase-1","receipt_kind":"skill","submitted_receipts":[]})).unwrap()
}
#[test]
fn receipt_diff_read_uses_exact_stored_phase_and_full_unpinned_first_read() {
    let context = context();
    let query = query(&context);
    let port = FixturePort(Mutex::new(vec![]));
    let read = query.resolve_receipt_diff(&context, &port).unwrap();
    assert_eq!(read.run_id, context.run.id);
    assert_eq!(read.definition_digest, context.run.definition_digest);
    assert_eq!(read.phase_id, "phase-1");
    assert_eq!(read.submitted_digest, "ab".repeat(32));
    assert_eq!(read.diff.missing.len(), 1);
    assert_eq!(read.diff.missing[0].instruction_id, "skill");
    assert_eq!(
        port.0.lock().unwrap()[0],
        br#"{"discriminator":"tectd.receipt-multiset.v1","receipt_kind":"skill","entries":[]}"#
    );
    let mut bad = query.clone();
    bad.definition_digest = Some("wrong".into());
    bad.phase_id = Some("wrong".into());
    assert_eq!(
        bad.resolve_receipt_diff(&context, &port)
            .unwrap_err()
            .refusal()
            .unwrap()
            .path
            .as_deref(),
        Some("arguments.params.definition_digest")
    );
    bad.definition_digest = None;
    assert_eq!(
        bad.resolve_receipt_diff(&context, &port).unwrap_err(),
        Error::NotFound
    );
    bad = query.clone();
    bad.submitted_digest = Some("ff".repeat(32));
    assert_eq!(
        bad.resolve_receipt_diff(&context, &port)
            .unwrap_err()
            .refusal()
            .unwrap()
            .path
            .as_deref(),
        Some("arguments.params.submitted_digest")
    );
}
#[test]
fn receipt_multiset_material_is_sorted_counted_exact_and_kind_sensitive() {
    let port = FixturePort(Mutex::new(vec![]));
    let receipt = |id: &str| PipelineSkillReadReceipt {
        instruction_id: id.into(),
        version: "v@界".into(),
        digest: "d🙂".into(),
    };
    let original = vec![receipt("z"), receipt("a@界"), receipt("z")];
    let mut reordered = original.clone();
    reordered.reverse();
    for submitted in [&original, &reordered] {
        crate::pipeline_receipt_multiset_digest(PipelineReceiptKind::Skill, submitted, &port)
            .unwrap();
    }
    let bytes = port.0.lock().unwrap();
    assert_eq!(bytes[0], bytes[1]);
    let material: serde_json::Value = serde_json::from_slice(&bytes[0]).unwrap();
    assert_eq!(material["entries"][0]["instruction_id"], "a@界");
    assert_eq!(material["entries"][1]["count"], 2);
    drop(bytes);
    crate::pipeline_receipt_multiset_digest(PipelineReceiptKind::Resource, &original, &port)
        .unwrap();
    let mut changed = original.clone();
    changed.pop();
    crate::pipeline_receipt_multiset_digest(PipelineReceiptKind::Skill, &changed, &port).unwrap();
    for field in 0..3 {
        let mut changed = original.clone();
        match field {
            0 => changed[0].instruction_id.push('x'),
            1 => changed[0].version.push('x'),
            _ => changed[0].digest.push('x'),
        };
        crate::pipeline_receipt_multiset_digest(PipelineReceiptKind::Skill, &changed, &port)
            .unwrap();
    }
    let bytes = port.0.lock().unwrap();
    for changed in &bytes[2..] {
        assert_ne!(&bytes[0], changed);
    }
}
#[test]
fn receipt_diff_continuation_requires_all_pins_and_wrong_view_fields_are_precise() {
    let context = context();
    let original = query(&context);
    for (offset, rep) in [(Some(1), None), (None, Some("aa".repeat(32)))] {
        let mut query = original.clone();
        query.offset_bytes = offset;
        query.representation_digest = rep;
        assert_eq!(
            query
                .validate()
                .unwrap_err()
                .refusal()
                .unwrap()
                .path
                .as_deref(),
            Some("arguments.params.definition_digest")
        );
        query.definition_digest = Some(context.run.definition_digest.clone());
        assert_eq!(
            query
                .validate()
                .unwrap_err()
                .refusal()
                .unwrap()
                .path
                .as_deref(),
            Some("arguments.params.submitted_digest")
        );
        query.submitted_digest = Some("ab".repeat(32));
        if query.representation_digest.is_none() {
            assert_eq!(
                query
                    .validate()
                    .unwrap_err()
                    .refusal()
                    .unwrap()
                    .path
                    .as_deref(),
                Some("arguments.params.representation_digest")
            );
            query.representation_digest = Some("aa".repeat(32));
        }
        query.validate().unwrap();
    }
    for field in ["receipt_kind", "submitted_receipts", "submitted_digest"] {
        let value = match field {
            "receipt_kind" => serde_json::json!("skill"),
            "submitted_receipts" => serde_json::json!([]),
            _ => serde_json::json!("ab".repeat(32)),
        };
        let mut encoded = serde_json::json!({"run_id":context.run.id,"view":"current"});
        encoded[field] = value;
        let query: PipelineRunContextQuery = serde_json::from_value(encoded).unwrap();
        assert_eq!(
            query.validate().unwrap_err().refusal().unwrap().path,
            Some(format!("arguments.params.{field}"))
        );
    }
    let mut query = original;
    query.submitted_receipts = None;
    assert_eq!(
        query
            .validate()
            .unwrap_err()
            .refusal()
            .unwrap()
            .path
            .as_deref(),
        Some("arguments.params.submitted_receipts")
    );
}
