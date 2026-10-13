//! How a review states a departure from Jev's advice, and what it may not state.
mod gate;

use super::*;
use crate::{CandidateReviewDraft, ReviewCandidateSet, ReviewSliceCandidateSet};
use crate::{SliceCandidateReviewDraft, SlicePlanReviewVerdict};
use serde_json::json;

fn departure(candidate: u128, point: BindingPoint, reason: &str) -> AdviceDeparture {
    AdviceDeparture {
        candidate_id: Uuid::from_u128(candidate),
        point,
        reason: reason.into(),
    }
}

/// A Scope review request and a Slice review request as an agent sends them, without
/// any departure.
fn requests() -> (Value, Value) {
    let id = Uuid::from_u128(9);
    let scope = json!({"candidate_set_id":id,"revision":2,"snapshot_id":id,"input_cursor":1,
        "request_id":id,"review":{"verdict":"ready","summary":"Bounded and ready",
            "findings":[],"candidate_decisions":[{"candidate_id":id,"decision":"accept",
                "rationale":"Coherent"}],"protected_change_reviews":[]}});
    let slice = json!({"scope_id":id,"candidate_set_id":id,"revision":2,"snapshot_id":id,
        "input_cursor":1,"request_id":id,
        "review":{"verdict":"ready","summary":"Bounded and ready","findings":[]}});
    (scope, slice)
}

#[test]
fn a_review_without_the_list_reads_and_writes_as_before() {
    let (scope, slice) = requests();
    let read: ReviewCandidateSet = serde_json::from_value(scope.clone()).unwrap();
    assert_eq!(read.review.advice_departures, None);
    // The request is what a receipt stores: it has no new key.
    assert_eq!(json!(read), scope);
    let read: ReviewSliceCandidateSet = serde_json::from_value(slice.clone()).unwrap();
    assert_eq!(read.review.advice_departures, None);
    assert_eq!(json!(read), slice);
    assert!(!json!(read).to_string().contains("advice_departures"));
}

#[test]
fn a_review_states_departures_as_a_list_of_candidate_point_and_reason() {
    let stated = json!([
        {"candidate_id":Uuid::from_u128(1),"point":"sizing","reason":"It is one outcome"},
        {"candidate_id":Uuid::from_u128(1),"point":"pipeline","reason":"The cause is known"}
    ]);
    let expected = vec![
        departure(1, BindingPoint::Sizing, "It is one outcome"),
        departure(1, BindingPoint::Pipeline, "The cause is known"),
    ];
    let (mut scope, mut slice) = requests();
    scope["review"]["advice_departures"] = stated.clone();
    slice["review"]["advice_departures"] = stated.clone();
    let read: ReviewCandidateSet = serde_json::from_value(scope.clone()).unwrap();
    assert_eq!(read.review.advice_departures.as_ref(), Some(&expected));
    assert_eq!(json!(read), scope);
    let read: ReviewSliceCandidateSet = serde_json::from_value(slice.clone()).unwrap();
    assert_eq!(read.review.advice_departures.as_ref(), Some(&expected));
    assert_eq!(json!(read), slice);
    assert_eq!(validate_advice_departures(&expected), Ok(()));
    assert_eq!(BindingPoint::Sizing.as_str(), "sizing");
    assert_eq!(BindingPoint::Pipeline.as_str(), "pipeline");

    // A stated empty list is a list, and it stays one.
    scope["review"]["advice_departures"] = json!([]);
    let read: ReviewCandidateSet = serde_json::from_value(scope.clone()).unwrap();
    assert_eq!(read.review.advice_departures, Some(Vec::new()));
    assert_eq!(json!(read), scope);
}

#[test]
fn null_an_unknown_point_and_an_unknown_key_do_not_read_as_departures() {
    let id = Uuid::from_u128(1);
    for stated in [
        Value::Null,
        json!({"candidate_id":id,"point":"sizing","reason":"r"}),
        json!([{"candidate_id":id,"point":"intake","reason":"r"}]),
        json!([{"candidate_id":id,"point":"size","reason":"r"}]),
        json!([{"candidate_id":id,"point":"sizing"}]),
        json!([{"point":"sizing","reason":"r"}]),
        json!([{"candidate_id":id,"point":"sizing","reason":"r","revision":1}]),
        json!([{"candidate_id":"first","point":"sizing","reason":"r"}]),
    ] {
        let (mut scope, mut slice) = requests();
        scope["review"]["advice_departures"] = stated.clone();
        slice["review"]["advice_departures"] = stated.clone();
        assert!(
            serde_json::from_value::<ReviewCandidateSet>(scope).is_err(),
            "{stated}"
        );
        assert!(
            serde_json::from_value::<ReviewSliceCandidateSet>(slice).is_err(),
            "{stated}"
        );
    }
    // The review bodies themselves take the same list and refuse the same null.
    let body = json!({"verdict":"ready","summary":"s","findings":[],"candidate_decisions":[],
        "advice_departures":null});
    assert!(serde_json::from_value::<CandidateReviewDraft>(body).is_err());
    let body = json!({"verdict":"revise","summary":"s","advice_departures":[]});
    let read: SliceCandidateReviewDraft = serde_json::from_value(body).unwrap();
    assert_eq!(read.verdict, SlicePlanReviewVerdict::Revise);
    assert_eq!(read.advice_departures, Some(Vec::new()));
}

#[test]
fn a_departure_gives_a_reason_and_names_a_candidate_and_point_once() {
    let pointer = |error: Error| {
        assert_eq!(error.code(), "invalid_arguments");
        let diagnostic = error.argument_diagnostic().unwrap().clone();
        (diagnostic.pointer, diagnostic.reason)
    };
    for blank in ["", "   ", "\n\t"] {
        let stated = [
            departure(1, BindingPoint::Sizing, "It is one outcome"),
            departure(2, BindingPoint::Sizing, blank),
        ];
        assert_eq!(
            pointer(validate_advice_departures(&stated).unwrap_err()),
            (
                "/params/review/advice_departures/1/reason".into(),
                "advice departure at index 1 requires a nonblank reason".into()
            )
        );
    }
    let twice = [
        departure(1, BindingPoint::Sizing, "It is one outcome"),
        departure(2, BindingPoint::Sizing, "Another"),
        departure(1, BindingPoint::Sizing, "Said again"),
    ];
    assert_eq!(
        pointer(validate_advice_departures(&twice).unwrap_err()),
        (
            "/params/review/advice_departures/2".into(),
            format!(
                "advice departure at index 2 repeats candidate {} and point sizing",
                Uuid::from_u128(1)
            )
        )
    );
    // One candidate under two points, and one point for two candidates, are not repeats.
    let apart = [
        departure(1, BindingPoint::Sizing, "a"),
        departure(1, BindingPoint::Pipeline, "b"),
        departure(2, BindingPoint::Pipeline, "c"),
    ];
    assert_eq!(validate_advice_departures(&apart), Ok(()));
    assert_eq!(validate_advice_departures(&[]), Ok(()));
}
