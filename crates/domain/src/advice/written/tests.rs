//! The one rule of "created or changed by this save", on both planning levels.
use super::super::test_drafts::{stored_scopes, stored_slices};
use super::*;
use serde_json::{Value, json};

fn scope_draft(identities: &[Value]) -> ScopeCandidateDraft {
    let candidates = identities
        .iter()
        .map(|identity| {
            json!({"identity":identity,"title":"t","outcome":"o","trigger":"t",
                "delivered_behavior":"b","proof":"p","coverage_goals":[]})
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({"boundary":"ongoing","goals":[],"candidates":candidates}))
        .unwrap()
}

/// A Slice candidate draft: each node is its kind and how the draft names it.
fn slice_draft(nodes: &[(&str, Value)]) -> SliceCandidateDraft {
    let nodes = nodes
        .iter()
        .map(|(kind, identity)| match *kind {
            "work" => json!({"kind":"work","identity":identity,"title":"t","outcome":"o",
                "proof":["p"],"pipeline":"slice.lightweight-tdd-development",
                "pipeline_reason":"r"}),
            _ => json!({"kind":"decision","identity":identity,"title":"t","question":"q",
                "resolution_criteria":["c"]}),
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({"coverage_summary":"s","nodes":nodes})).unwrap()
}

fn positions(written: &[Written]) -> Vec<usize> {
    written.iter().map(|written| written.position).collect()
}

#[test]
fn a_scope_candidate_is_written_when_it_is_new_or_stored_under_a_higher_revision() {
    let (kept, changed, new) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let draft = scope_draft(&[
        json!({"id":kept,"revision":4}),
        json!({"local":"added"}),
        json!({"id":changed,"revision":2}),
    ]);
    let stored = stored_scopes(&[(kept, 4), (new, 1), (changed, 3)]);
    let written = scope_candidates(&draft, &stored).unwrap();
    // Positions count every candidate of the draft; each written one carries the id
    // and the revision the store holds it under, the new one the id the store gave it.
    assert_eq!(
        written,
        [
            Written {
                position: 2,
                id: new,
                revision: 1
            },
            Written {
                position: 3,
                id: changed,
                revision: 3
            }
        ]
    );

    // A draft that keeps every candidate it names wrote none.
    let draft = scope_draft(&[json!({"id":kept,"revision":4})]);
    assert_eq!(
        scope_candidates(&draft, &stored_scopes(&[(kept, 4)])),
        Ok(vec![])
    );
    assert_eq!(
        scope_candidates(&scope_draft(&[]), &stored_scopes(&[])),
        Ok(vec![])
    );
}

#[test]
fn a_slice_work_node_is_written_by_the_same_rule_and_a_decision_node_never() {
    let (kept, changed, decided) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let draft = slice_draft(&[
        ("work", json!({"local":"form"})),
        ("decision", json!({"local":"mail"})),
        ("work", json!({"candidate_id":kept,"revision":1})),
        ("work", json!({"candidate_id":changed,"revision":2})),
        ("decision", json!({"candidate_id":decided,"revision":1})),
        ("work", json!({"local":"retry"})),
    ]);
    let stored = stored_slices(&[
        (ids[0], 1),
        (ids[1], 1),
        (kept, 1),
        (changed, 3),
        (decided, 2),
        (ids[2], 1),
    ]);
    let written = slice_work_nodes(&draft, &stored).unwrap();
    // The new decision node at 2 and the changed one at 5 are not among them, nor
    // is the kept work node at 3.
    assert_eq!(positions(&written), [1, 4, 6]);
    assert_eq!(
        written
            .iter()
            .map(|written| (written.id, written.revision))
            .collect::<Vec<_>>(),
        [(ids[0], 1), (changed, 3), (ids[2], 1)]
    );

    // Decision nodes alone, new or changed, and a kept work node beside them: none.
    let draft = slice_draft(&[
        ("decision", json!({"local":"mail"})),
        ("work", json!({"candidate_id":kept,"revision":1})),
        ("decision", json!({"candidate_id":decided,"revision":1})),
    ]);
    let stored = stored_slices(&[(ids[0], 1), (kept, 1), (decided, 2)]);
    assert_eq!(slice_work_nodes(&draft, &stored), Ok(vec![]));
}

#[test]
fn a_stored_draft_the_save_cannot_have_left_is_an_internal_error() {
    let named = Uuid::new_v4();
    let draft = scope_draft(&[json!({"local":"added"}), json!({"id":named,"revision":1})]);
    // The store holds fewer candidates than the draft, or more.
    for kept in [vec![(Uuid::new_v4(), 1)], vec![(named, 1); 3]] {
        assert_eq!(
            scope_candidates(&draft, &stored_scopes(&kept)),
            Err(Error::InternalInvariant)
        );
    }
    // The store holds another candidate where the draft names one by id.
    let other = stored_scopes(&[(Uuid::new_v4(), 1), (Uuid::new_v4(), 2)]);
    assert_eq!(
        scope_candidates(&draft, &other),
        Err(Error::InternalInvariant)
    );
    // A candidate without a label that names no revision: the save refused that draft.
    let mut unnamed = draft.clone();
    unnamed.candidates[1].identity.revision = None;
    let stored = stored_scopes(&[(Uuid::new_v4(), 1), (named, 1)]);
    assert_eq!(
        scope_candidates(&unnamed, &stored),
        Err(Error::InternalInvariant)
    );

    let draft = slice_draft(&[("work", json!({"candidate_id":named,"revision":1}))]);
    assert_eq!(
        slice_work_nodes(&draft, &stored_slices(&[])),
        Err(Error::InternalInvariant)
    );
    assert_eq!(
        slice_work_nodes(&draft, &stored_slices(&[(Uuid::new_v4(), 2)])),
        Err(Error::InternalInvariant)
    );
}
