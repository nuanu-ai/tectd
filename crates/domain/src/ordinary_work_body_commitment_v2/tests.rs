use super::*;
use crate::{ModelRouteCallerFacts, PipelineCheckpointRef, PipelineKind};
use serde_json::json;
use uuid::Uuid;

// Independent literal oracle: see I1177 receipt for external shasum command.
const EXPECTED: &str = r#"{"schema":"ordinary-model-route/work-body/v2","work":{"dependencies":["00000000-0000-0000-0000-000000000002","00000000-0000-0000-0000-000000000003"],"excludes":["x","y"],"id":"00000000-0000-0000-0000-000000000001","includes":["a","β"],"kind":"work","model_route_facts":{"available_latency_ms":20,"data_class":"internal","remaining_budget_units":10,"role":"agent","tool":"code"},"outcome":"готово","pipeline":"slice.lightweight-tdd-development","pipeline_reason":"small","proof":["p","q"],"revision":7,"source_checkpoint":{"checkpoint_id":"00000000-0000-0000-0000-000000000004","digest":"checkpoint"},"source_result_ids":["00000000-0000-0000-0000-000000000005","00000000-0000-0000-0000-000000000006"],"title":"тест 🌱","why_further_vertical_split_not_viable":"atomic","why_lightweight_insufficient":"reason"}}"#;
const EXPECTED_SHA256: &str = "dbac5a5e4e31a6fe6ad3c561217bb6810a2b04ee4ac756971fb125825c346276";

fn fixture() -> SliceCandidateNode {
    SliceCandidateNode::Work {
        id: Uuid::from_u128(1),
        revision: 7,
        model_route_facts: Some(Box::new(ModelRouteCallerFacts {
            role: Some("agent".into()),
            tool: Some("code".into()),
            data_class: Some("internal".into()),
            remaining_budget_units: Some(10),
            available_latency_ms: Some(20),
        })),
        title: "тест 🌱".into(),
        outcome: "готово".into(),
        includes: vec!["a".into(), "β".into()],
        excludes: vec!["x".into(), "y".into()],
        dependencies: vec![Uuid::from_u128(2), Uuid::from_u128(3)],
        proof: vec!["p".into(), "q".into()],
        pipeline: PipelineKind::LightweightTddDevelopment,
        pipeline_reason: "small".into(),
        why_lightweight_insufficient: Some("reason".into()),
        why_further_vertical_split_not_viable: Some("atomic".into()),
        source_result_ids: vec![Uuid::from_u128(5), Uuid::from_u128(6)],
        source_checkpoint: Some(PipelineCheckpointRef {
            checkpoint_id: Uuid::from_u128(4),
            digest: "checkpoint".into(),
        }),
    }
}

fn commit(value: Value) -> OrdinaryWorkBodyCommitmentV2 {
    commit_ordinary_work_body_v2(&serde_json::from_value(value).unwrap()).unwrap()
}

fn assert_different(a: Value, b: Value, label: &str) {
    let a = commit(a);
    let b = commit(b);
    assert_ne!(a.work(), b.work(), "{label}");
    assert_ne!(a.canonical_bytes(), b.canonical_bytes(), "{label}");
    assert_ne!(a.digest(), b.digest(), "{label}");
}

#[test]
fn full_literal_oracle_unicode_and_determinism() {
    let node = fixture();
    let committed = commit_ordinary_work_body_v2(&node).unwrap();
    assert_eq!(committed.work(), &node);
    assert_eq!(committed.canonical_bytes(), EXPECTED.as_bytes());
    assert_eq!(committed.digest(), EXPECTED_SHA256);
    assert_eq!(committed, commit_ordinary_work_body_v2(&node).unwrap());
    assert_eq!(committed, committed.clone());
}

#[test]
fn every_work_field_is_committed() {
    let original = serde_json::to_value(fixture()).unwrap();
    let changes = [
        ("id", json!(Uuid::from_u128(99))),
        ("revision", json!(-8)),
        ("model_route_facts", json!({})),
        ("title", json!("other")),
        ("outcome", json!("other")),
        ("includes", json!(["other"])),
        ("excludes", json!(["other"])),
        ("dependencies", json!([Uuid::from_u128(99)])),
        ("proof", json!(["other"])),
        ("pipeline", json!(PipelineKind::Research)),
        ("pipeline_reason", json!("other")),
        ("why_lightweight_insufficient", json!("other")),
        ("why_further_vertical_split_not_viable", json!("other")),
        ("source_result_ids", json!([Uuid::from_u128(99)])),
        ("source_checkpoint", Value::Null),
    ];
    assert_eq!(changes.len(), 15);
    for (field, replacement) in changes {
        let mut changed = original.clone();
        changed[field] = replacement;
        assert_different(original.clone(), changed, field);
    }
}

#[test]
fn every_caller_fact_value_and_absence_is_committed() {
    let original = serde_json::to_value(fixture()).unwrap();
    for (field, replacement) in [
        ("role", json!("other")),
        ("tool", json!("other")),
        ("data_class", json!("other")),
        ("remaining_budget_units", json!(11)),
        ("available_latency_ms", json!(21)),
    ] {
        let mut changed = original.clone();
        changed["model_route_facts"][field] = replacement;
        assert_different(original.clone(), changed, field);
        let mut absent = original.clone();
        absent["model_route_facts"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert_different(original.clone(), absent, field);
    }
    let mut absent = original.clone();
    absent.as_object_mut().unwrap().remove("model_route_facts");
    let mut empty = absent.clone();
    empty["model_route_facts"] = json!({});
    assert_different(absent.clone(), empty.clone(), "None versus Some(Default)");
    for field in ["remaining_budget_units", "available_latency_ms"] {
        let mut zero = empty.clone();
        zero["model_route_facts"][field] = json!(0);
        assert_different(empty.clone(), zero, field);
    }
    assert_different(original, absent, "whole facts absence");
}

#[test]
fn checkpoint_fields_and_absence_are_committed() {
    let original = serde_json::to_value(fixture()).unwrap();
    for (field, replacement) in [
        ("checkpoint_id", json!(Uuid::nil())),
        ("digest", json!("other")),
    ] {
        let mut changed = original.clone();
        changed["source_checkpoint"][field] = replacement;
        assert_different(original.clone(), changed, field);
    }
    let mut absent = original.clone();
    absent.as_object_mut().unwrap().remove("source_checkpoint");
    assert_different(original, absent, "checkpoint absence");
}

#[test]
fn why_none_differs_from_some_empty() {
    let original = serde_json::to_value(fixture()).unwrap();
    for field in [
        "why_lightweight_insufficient",
        "why_further_vertical_split_not_viable",
    ] {
        let mut absent = original.clone();
        absent[field] = Value::Null;
        let mut empty = absent.clone();
        empty[field] = json!("");
        assert_different(absent.clone(), empty, field);
        let bytes = commit(absent);
        let envelope: Value = serde_json::from_slice(bytes.canonical_bytes()).unwrap();
        assert!(envelope["work"].as_object().unwrap().contains_key(field));
        assert!(envelope["work"][field].is_null());
    }
}

#[test]
fn all_five_array_orders_are_committed() {
    let original = serde_json::to_value(fixture()).unwrap();
    for field in [
        "includes",
        "excludes",
        "dependencies",
        "proof",
        "source_result_ids",
    ] {
        let mut reversed = original.clone();
        let values = reversed[field].as_array_mut().unwrap();
        assert_eq!(values.len(), 2);
        assert_ne!(values[0], values[1]);
        values.reverse();
        assert_different(original.clone(), reversed, field);
    }
}

#[test]
fn recursive_objects_sort_and_arrays_retain_order() {
    let value = json!({"z": [{"b": "β", "a": 2}, "тест"], "a": {"z": 3, "a": 1}});
    let expected = r#"{"a":{"a":1,"z":3},"z":[{"a":2,"b":"β"},"тест"]}"#;
    assert_eq!(canonical_bytes(value).unwrap(), expected.as_bytes());
}

#[test]
fn original_mutation_cannot_change_stored_commitment() {
    let mut original = fixture();
    let committed = commit_ordinary_work_body_v2(&original).unwrap();
    let before = committed.clone();
    if let SliceCandidateNode::Work {
        title,
        model_route_facts,
        includes,
        source_checkpoint,
        ..
    } = &mut original
    {
        *title = "changed".into();
        model_route_facts.as_mut().unwrap().role = Some("changed".into());
        includes.reverse();
        source_checkpoint.as_mut().unwrap().digest = "changed".into();
    }
    assert_ne!(committed.work(), &original);
    assert_eq!(committed.work(), before.work());
    assert_eq!(committed.canonical_bytes(), before.canonical_bytes());
    assert_eq!(committed.digest(), before.digest());
}

#[test]
fn decisions_are_rejected() {
    let decision = SliceCandidateNode::Decision {
        id: Uuid::nil(),
        revision: 0,
        title: "decision".into(),
        question: "?".into(),
        resolution_criteria: vec![],
        dependencies: vec![],
        source_result_ids: vec![],
    };
    assert_eq!(
        commit_ordinary_work_body_v2(&decision),
        Err(Error::InvalidArguments)
    );
}
