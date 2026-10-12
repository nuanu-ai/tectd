use super::*;
use crate::json_fragment::READ_BUDGET;
use serde_json::json;
use tect_domain::{AdviceShaping, AdviceUnavailableReason, SizingAdvice, SizingDraft, SizingScope};

/// The `sizing` point for a Scope candidate draft of this shape.
pub(crate) fn scope_sizing(candidates: usize, items: usize) -> SizingAdvice {
    let candidates = (0..candidates)
        .map(|index| {
            json!({"identity":{"local":format!("c{index}")},"title":"t","outcome":"o",
                "trigger":"t","delivered_behavior":"b","proof":"p",
                "includes":vec!["i"; items],"coverage_goals":[]})
        })
        .collect::<Vec<_>>();
    let draft = serde_json::from_value(json!({"boundary":"ongoing","goals":[],
        "candidates":candidates}))
    .unwrap();
    SizingDraft::scope_candidates(&draft)
        .unwrap()
        .unwrap()
        .advice(&[])
}

/// The `sizing` point for a Slice candidate draft of this shape.
pub(crate) fn slice_sizing(candidates: usize, items: usize) -> SizingAdvice {
    let nodes = (0..candidates)
        .map(|index| {
            json!({"kind":"work","identity":{"local":format!("c{index}")},"title":"t",
                "outcome":"o","includes":vec!["i"; items],"proof":["p"],
                "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r"})
        })
        .collect::<Vec<_>>();
    let draft = serde_json::from_value(json!({"coverage_summary":"s","nodes":nodes})).unwrap();
    let scope = SizingScope {
        title: "t".into(),
        outcome: "o".into(),
        includes: Vec::new(),
        excludes: Vec::new(),
    };
    SizingDraft::slice_candidates(&draft, scope)
        .unwrap()
        .unwrap()
        .advice(&[])
}

/// How many candidates with `items` included items each a save reply can carry advice
/// for, and what one more candidate costs in reply bytes: measured, not computed.
pub(crate) fn most_candidates(
    reply: &Value,
    sizing: impl Fn(usize, usize) -> SizingAdvice,
    items: usize,
) -> (usize, usize) {
    let room = ReplyRoom {
        reply,
        capacity: READ_BUDGET,
    };
    let bytes = |count: usize| {
        let mut with = reply.clone();
        with["advice"] = json!(sizing(count, items).largest());
        responses::encoded_len(&with).unwrap()
    };
    let mut fitting = 0;
    while room
        .check_reply(&sizing(fitting + 1, items).largest())
        .is_ok()
    {
        fitting += 1;
    }
    assert!(fitting >= 1, "not even one candidate fits");
    assert!(bytes(fitting) <= READ_BUDGET && bytes(fitting + 1) > READ_BUDGET);
    assert_eq!(
        room.check_reply(&sizing(fitting + 1, items).largest()),
        Err(Error::RequestTooLarge)
    );
    (fitting, bytes(2) - bytes(1))
}

fn reply() -> Value {
    responses::with_actions(
        json!({"operation":"saved","counts":{"candidates":2}}),
        vec![],
        None,
    )
}

#[test]
fn a_reply_without_a_field_is_returned_untouched() {
    let reply = reply();
    let bytes = serde_json::to_vec(&reply).unwrap();
    // Even a capacity the reply itself would not pass: `main` already judged it.
    for capacity in [READ_BUDGET, 1] {
        let finished = finish(reply.clone(), None, capacity).unwrap();
        assert_eq!(serde_json::to_vec(&finished).unwrap(), bytes);
    }
}

#[test]
fn the_field_is_attached_under_advice_and_held_to_the_reply_budget() {
    let reply = reply();
    let advice = scope_sizing(2, 3).largest();
    let finished = finish(reply.clone(), Some(advice.clone()), READ_BUDGET).unwrap();
    assert_eq!(finished["advice"], json!(advice));
    assert_eq!(finished["advice"]["status"], "advised");
    let mut without = finished.clone();
    without.as_object_mut().unwrap().remove("advice");
    assert_eq!(without, reply);

    // The guard and the reply that leaves use one measure: they agree at every capacity.
    let exact = responses::encoded_len(&finished).unwrap();
    for (capacity, fits) in [(exact, true), (exact - 1, false), (usize::MAX, true)] {
        let room = ReplyRoom {
            reply: &reply,
            capacity,
        };
        assert_eq!(room.check_reply(&advice).is_ok(), fits, "{capacity}");
        assert_eq!(
            finish(reply.clone(), Some(advice.clone()), capacity).is_ok(),
            fits
        );
    }
    // A caller's larger capacity never lifts the read budget.
    let huge = scope_sizing(100, 100).largest();
    let room = ReplyRoom {
        reply: &reply,
        capacity: usize::MAX,
    };
    assert_eq!(room.check_reply(&huge), Err(Error::RequestTooLarge));
    assert_eq!(
        finish(reply.clone(), Some(huge), usize::MAX),
        Err(Error::RequestTooLarge)
    );

    // Any field goes through the same check, not only an advised one.
    let unavailable = AdviceField::unavailable(AdviceUnavailableReason::Timeout, "late");
    let finished = finish(reply.clone(), Some(unavailable), READ_BUDGET).unwrap();
    assert_eq!(finished["advice"]["reason"], "timeout");
    assert_eq!(
        finish(
            json!("not an object"),
            Some(AdviceField::Abstained),
            READ_BUDGET
        ),
        Err(Error::InternalInvariant)
    );
}
