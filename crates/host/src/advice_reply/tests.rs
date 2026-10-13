use super::*;
use crate::json_fragment::READ_BUDGET;
use serde_json::json;
use tect_domain::{
    AdviceShaping, AdviceUnavailableReason, IntakeAdvice, IntakePlanning, PipelineAdvice,
    PipelineCatalogueSnapshot, PipelineDraft, Program, ProgramMode, SizingAdvice, SizingDraft,
    SizingScope,
};
use uuid::Uuid;

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
        .advice(&[], ProgramMode::Prod)
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
        .advice(&[], ProgramMode::Prod)
}

/// The catalogue the daemon serves. Building it validates every static pipeline
/// definition, so a measurement builds it once and asks about many drafts.
pub(crate) fn served_catalogue() -> PipelineCatalogueSnapshot {
    crate::slice_pipeline_catalog::snapshot()
}

/// The `pipeline` point for a first Slice candidate draft of this many work nodes,
/// every one of them new and so asked about, against `catalogue`.
pub(crate) fn slice_pipeline(
    nodes: usize,
    catalogue: &PipelineCatalogueSnapshot,
) -> PipelineAdvice {
    let nodes = (0..nodes)
        .map(|index| {
            json!({"kind":"work","identity":{"local":format!("c{index}")},"title":"t",
                "outcome":"o","proof":["p"],
                "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r"})
        })
        .collect::<Vec<_>>();
    let draft = serde_json::from_value(json!({"coverage_summary":"s","nodes":nodes})).unwrap();
    // A new node is told by its label; the stored draft is read for named ids only.
    let stored =
        serde_json::from_value(json!({"coverage_summary":"s","nodes":[],"supersessions":[]}))
            .unwrap();
    let scope = SizingScope {
        title: "t".into(),
        outcome: "o".into(),
        includes: Vec::new(),
        excludes: Vec::new(),
    };
    PipelineDraft::slice_candidates(&draft, &stored, catalogue, scope)
        .unwrap()
        .unwrap()
        .advice(&[], ProgramMode::Prod)
}

fn asked_with(planning: IntakePlanning) -> IntakeAdvice {
    let id = Uuid::new_v4();
    planning.advice(&Program::draft(id, id, 0), ProgramMode::Prod, &[], "i")
}

/// The `intake` point for an input of a Scope candidate set whose saved draft has
/// this many candidates; `None` is a set without a saved draft.
pub(crate) fn scope_intake(candidates: Option<usize>) -> IntakeAdvice {
    let draft = candidates.map(|count| {
        let candidates = (0..count)
            .map(|_| {
                json!({"id":Uuid::new_v4(),"revision":1,"title":"t","outcome":"o","trigger":"t",
                    "delivered_behavior":"b","proof":"p","includes":[],"excludes":[],
                    "dependencies":[],"coverage_goal_ids":[],"evidence_ids":[]})
            })
            .collect::<Vec<_>>();
        serde_json::from_value(json!({"boundary":"ongoing","goals":[],"evidence":[],
            "candidates":candidates,"blockers":[],"protected_changes":[]}))
        .unwrap()
    });
    asked_with(IntakePlanning::scope_candidates(draft.as_ref()).unwrap())
}

/// The `intake` point for an input of a Slice candidate set whose saved draft has
/// this many work nodes; `None` is a set without a saved draft.
pub(crate) fn slice_intake(candidates: Option<usize>) -> IntakeAdvice {
    let draft = candidates.map(|count| {
        let nodes = (0..count)
            .map(|_| {
                json!({"kind":"work","id":Uuid::new_v4(),"revision":1,"title":"t","outcome":"o",
                    "includes":[],"excludes":[],"dependencies":[],"proof":["p"],
                    "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r",
                    "why_lightweight_insufficient":null,
                    "why_further_vertical_split_not_viable":null,"source_result_ids":[]})
            })
            .collect::<Vec<_>>();
        serde_json::from_value(json!({"coverage_summary":"s","nodes":nodes,"supersessions":[]}))
            .unwrap()
    });
    let scope = SizingScope {
        title: "t".into(),
        outcome: "o".into(),
        includes: Vec::new(),
        excludes: Vec::new(),
    };
    asked_with(IntakePlanning::slice_candidates(draft.as_ref(), scope).unwrap())
}

/// What the largest field of a point adds to a reply that has room for it: measured
/// with the reply's own encoder, not computed.
pub(crate) fn largest_bytes(reply: &Value, point: &dyn AdviceShaping) -> usize {
    let largest = point.largest();
    let room = ReplyRoom {
        reply,
        capacity: READ_BUDGET,
    };
    assert_eq!(room.check_reply(&largest), Ok(()));
    let with = finish(reply.clone(), Some(largest), READ_BUDGET).unwrap();
    responses::encoded_len(&with).unwrap() - responses::encoded_len(reply).unwrap()
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

/// How many asked work nodes the `pipeline_advice` field can cover on a Slice draft
/// save reply, which may already hold an `advice` field, and what one more node costs
/// in reply bytes: measured, not computed.
pub(crate) fn most_pipeline_nodes(reply: &Value) -> (usize, usize) {
    let catalogue = served_catalogue();
    let room = PipelineReplyRoom {
        reply,
        capacity: READ_BUDGET,
    };
    let bytes = |count: usize| {
        let mut with = reply.clone();
        with["pipeline_advice"] = json!(slice_pipeline(count, &catalogue).largest());
        responses::encoded_len(&with).unwrap()
    };
    let mut fitting = 0;
    while room
        .check_reply(&slice_pipeline(fitting + 1, &catalogue).largest())
        .is_ok()
    {
        fitting += 1;
    }
    assert!(fitting == 0 || bytes(fitting) <= READ_BUDGET);
    assert!(bytes(fitting + 1) > READ_BUDGET);
    assert_eq!(
        room.check_reply(&slice_pipeline(fitting + 1, &catalogue).largest()),
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

#[test]
fn the_pipeline_field_is_attached_under_its_own_key_beside_advice() {
    let sizing = slice_sizing(2, 3).largest();
    let reply = finish(reply(), Some(sizing.clone()), READ_BUDGET).unwrap();
    let bytes = serde_json::to_vec(&reply).unwrap();
    // No field from the point: the reply that holds `advice` leaves as it is.
    for capacity in [READ_BUDGET, 1] {
        let finished = finish_pipeline(reply.clone(), None, capacity).unwrap();
        assert_eq!(serde_json::to_vec(&finished).unwrap(), bytes);
    }

    let catalogue = served_catalogue();
    let pipeline = slice_pipeline(2, &catalogue).largest();
    let finished = finish_pipeline(reply.clone(), Some(pipeline.clone()), READ_BUDGET).unwrap();
    assert_eq!(finished["pipeline_advice"], json!(pipeline));
    assert_eq!(finished["advice"], json!(sizing));
    let mut without = finished.clone();
    without.as_object_mut().unwrap().remove("pipeline_advice");
    assert_eq!(serde_json::to_vec(&without).unwrap(), bytes);

    // The room is what the reply has left with the `advice` field it holds: the guard
    // and the reply that leaves agree at every capacity.
    let exact = responses::encoded_len(&finished).unwrap();
    for (capacity, fits) in [(exact, true), (exact - 1, false), (usize::MAX, true)] {
        let room = PipelineReplyRoom {
            reply: &reply,
            capacity,
        };
        assert_eq!(room.check_reply(&pipeline).is_ok(), fits, "{capacity}");
        assert_eq!(
            finish_pipeline(reply.clone(), Some(pipeline.clone()), capacity).is_ok(),
            fits
        );
    }
    let huge = slice_pipeline(100, &catalogue).largest();
    let room = PipelineReplyRoom {
        reply: &reply,
        capacity: usize::MAX,
    };
    assert_eq!(room.check_reply(&huge), Err(Error::RequestTooLarge));
    assert_eq!(
        finish_pipeline(reply.clone(), Some(huge), usize::MAX),
        Err(Error::RequestTooLarge)
    );

    // Any field goes through the same check, not only an advised one.
    let unavailable = AdviceField::unavailable(AdviceUnavailableReason::Timeout, "late");
    let finished = finish_pipeline(reply.clone(), Some(unavailable), READ_BUDGET).unwrap();
    assert_eq!(finished["pipeline_advice"]["reason"], "timeout");
    assert_eq!(finished["advice"], json!(sizing));
}
