//! Whom Jev is asked about: the candidates the save created or changed, while the
//! state still carries the whole draft.
use super::*;

fn asked(advice: &SizingAdvice) -> Vec<&String> {
    advice.questions().questions.keys().collect()
}

#[test]
fn a_scope_save_is_asked_about_the_candidates_it_created_or_changed_only() {
    let existing = Uuid::new_v4();
    let draft = scope_draft(existing);
    let whole = scope_advice_of(existing);

    // The save kept the candidate the draft names by id: only the new one is asked about.
    let kept = stored_scopes(&[(Uuid::from_u128(1), 1), (existing, 2)]);
    let advice = SizingDraft::scope_candidates(&draft, &kept)
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod);
    assert_eq!(asked(&advice), ["c1.advice", "c1.i1", "c1.i2", "c1.size"]);
    // Jev still reads every candidate of the draft.
    assert_eq!(advice.questions().state, whole.questions().state);
    assert_eq!(
        advice.questions().state["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        advice.subject(),
        "this draft (1 candidate, 2 included items)"
    );
    let largest = content(&advice.largest());
    assert_eq!(largest["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(largest["candidates"][0]["candidate"], 1);

    // The save changed the named candidate and the draft names both by id: the
    // changed one is asked about under its own draft position.
    let mut second = draft.clone();
    second.candidates[0].identity =
        serde_json::from_value(json!({"id":Uuid::from_u128(1),"revision":1})).unwrap();
    second.goals[0].resolution.reference =
        serde_json::from_value(json!({"id":Uuid::from_u128(1)})).unwrap();
    let changed = stored_scopes(&[(Uuid::from_u128(1), 1), (existing, 3)]);
    let advice = SizingDraft::scope_candidates(&second, &changed)
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod);
    assert_eq!(asked(&advice), ["c2.advice", "c2.size"]);
    let given = answers(&advice, |name| match name {
        "c2.size" => ("large", 0.6),
        _ => ("split", 0.8),
    });
    assert_eq!(
        content(&advice.shape(&given))["candidates"],
        json!([{"candidate":2,"size":"large","size_p":0.6,"advice":"split","advice_p":0.8,
            "extra_items":[],"extra_p":[]}])
    );

    // The save created and changed no candidate: the point does not fire.
    let unchanged = stored_scopes(&[(Uuid::from_u128(1), 1), (existing, 2)]);
    assert_eq!(SizingDraft::scope_candidates(&second, &unchanged), Ok(None));
}

#[test]
fn a_slice_save_is_asked_about_the_work_nodes_it_created_or_changed_only() {
    let existing = Uuid::new_v4();
    let draft = slice_draft(existing);
    let ask = |stored: &ResolvedSliceCandidateDraft| {
        SizingDraft::slice_candidates(&draft, stored, scope_statement())
    };

    // The save kept the work node the draft names by id: the new work node is asked
    // about, the kept one and the decision node are shown.
    let kept = stored_slices(&[
        (Uuid::from_u128(1), 1),
        (Uuid::from_u128(2), 1),
        (existing, 1),
    ]);
    let advice = ask(&kept)
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod);
    assert_eq!(asked(&advice), ["c1.advice", "c1.i1", "c1.size"]);
    assert_eq!(advice.questions().state, slice_advice().questions().state);
    assert_eq!(
        advice.subject(),
        "this draft (1 candidate, 1 included item)"
    );

    // A later save names all three by id and changes the third, which keeps position 3.
    let mut second = draft.clone();
    second.nodes[0] = serde_json::from_value(json!({"kind":"work",
        "identity":{"candidate_id":Uuid::from_u128(1),"revision":1},"title":"Invitation form",
        "outcome":"The form sends an invitation","includes":["form"],"proof":["p"],
        "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r"}))
    .unwrap();
    second.nodes[1] = serde_json::from_value(json!({"kind":"decision",
        "identity":{"candidate_id":Uuid::from_u128(2),"revision":1},"title":"t",
        "question":"q","resolution_criteria":["c"],
        "dependencies":[{"candidate_id":Uuid::from_u128(1),"revision":1}]}))
    .unwrap();
    second.nodes[2] = serde_json::from_value(json!({"kind":"work",
        "identity":{"candidate_id":existing,"revision":1},"title":"Delivery",
        "outcome":"The invitation arrives","includes":["sending","retries","metrics"],
        "dependencies":[{"candidate_id":Uuid::from_u128(2),"revision":1}],
        "proof":["p"],"pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r"}))
    .unwrap();
    let later = |stored: &ResolvedSliceCandidateDraft| {
        SizingDraft::slice_candidates(&second, stored, scope_statement())
    };
    let changed = stored_slices(&[
        (Uuid::from_u128(1), 1),
        (Uuid::from_u128(2), 1),
        (existing, 2),
    ]);
    let advice = later(&changed)
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod);
    assert_eq!(
        asked(&advice),
        ["c3.advice", "c3.i1", "c3.i2", "c3.i3", "c3.size"]
    );
    assert_eq!(
        content(&advice.largest())["candidates"],
        json!([{"candidate":3,"size":"oversized","size_p":0.99,"advice":"separate_scope",
            "advice_p":0.99,"extra_items":[1,2,3],"extra_p":[0.99,0.99,0.99]}])
    );

    // The save changed the decision node alone, or nothing: no work node was
    // written, and the point does not fire.
    let decided = stored_slices(&[
        (Uuid::from_u128(1), 1),
        (Uuid::from_u128(2), 2),
        (existing, 1),
    ]);
    assert_eq!(later(&decided), Ok(None));
    assert_eq!(later(&kept), Ok(None));
}
