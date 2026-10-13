//! How much advice the replies of the Slice candidate commands can carry: `sizing` and
//! beside it `pipeline` on a draft save, `intake` on a recorded input. Measured with
//! the real reply and the real encoder.
use super::*;
use crate::advice_reply::tests::{
    largest_bytes, most_candidates, most_pipeline_nodes, served_catalogue, slice_intake,
    slice_pipeline, slice_sizing,
};
use crate::advice_reply::{PipelineReplyRoom, finish, finish_pipeline};
use crate::json_fragment::READ_BUDGET;
use tect_application::AdviceOutputGuard;
use tect_domain::{AdviceField, AdviceShaping, AdviceUnavailableReason, Error};

/// A Slice planning context as it stands right after a draft was stored.
fn saved() -> SliceCandidateContext {
    let id = Uuid::new_v4();
    serde_json::from_value(json!({
        "scope":{"id":id,"workspace_id":id,"revision":3,"source_candidate_set_id":id,
            "source_candidate_set_revision":3,"source_snapshot_id":id,"source_candidate_id":id,
            "source_candidate_revision":3,"boundary":"finite","title":"Scope","outcome":"Observed",
            "includes":[],"excludes":[],"slice_candidate_set_id":id,"slice_input_cursor":3,
            "slice_latest_input":3},
        "candidate_set":{"id":id,"scope_id":id,"revision":3,"status":"review_required",
            "current_snapshot_id":id,"input_cursor":3,"latest_input":3},
        "snapshot":{"id":id,"sequence":3,"scope_revision":3,"source_candidate_set_revision":3,
            "source_snapshot_id":id,"planning_latest_input":3,
            "method":{"id":"method","revision":"1","digest":"d","body":"b","origin_refs":["t"]},
            "registry_revision":"1","registry_digest":"d","rules":[],
            "catalogue":{"revision":"4","digest":"d","entries":[]},"result_ids":[]},
        "draft":{"coverage_summary":"summary","nodes":[],"supersessions":[]},
        "reviews":[],"inputs":[],"history":[],"slices":[],"results":[],"stale_reasons":[]
    }))
    .unwrap()
}

#[test]
fn draft_save_reply_has_measured_room_for_sizing_advice() {
    let reply = candidate_mutation(saved()).unwrap();
    let base = responses::encoded_len(&reply).unwrap();
    let (bare, each) = most_candidates(&reply, slice_sizing, 0);
    let (usual, _) = most_candidates(&reply, slice_sizing, 5);
    let (heavy, _) = most_candidates(&reply, slice_sizing, 100);
    println!(
        "slice.candidates.save base_envelope_bytes={base}; sizing advice bytes_per_candidate={each}; \
         candidates_that_fit with 0/5/100 included items each={bare}/{usual}/{heavy}"
    );
    assert_eq!((base, each), (1373, 139));
    assert_eq!((bare, usual, heavy), (46, 37, 7));
}

#[test]
fn record_input_reply_has_measured_room_for_intake_advice() {
    // The reply of a recorded input is built as the reply of a stored draft is.
    let reply = candidate_mutation(saved()).unwrap();
    let base = responses::encoded_len(&reply).unwrap();
    let bare = largest_bytes(&reply, &slice_intake(None));
    let named = largest_bytes(&reply, &slice_intake(Some(120)));
    println!(
        "slice.candidates.input base_envelope_bytes={base}; largest intake advice bytes \
         without a saved draft={bare}, with one={named}"
    );
    assert_eq!((base, bare, named), (1373, 551, 594));
}

/// The reply with the largest `sizing` field a draft of this shape can get.
fn with_full_sizing(reply: &Value, candidates: usize, items: usize) -> Result<Value> {
    let largest = slice_sizing(candidates, items).largest();
    finish(reply.clone(), Some(largest), READ_BUDGET)
}

/// The largest first draft, every node of it new work with `items` included items,
/// whose largest `sizing` field and largest `pipeline_advice` field both fit the reply.
fn most_nodes_with_both(reply: &Value, items: usize) -> usize {
    let catalogue = served_catalogue();
    let mut fitting = 0;
    while let Ok(sized) = with_full_sizing(reply, fitting + 1, items) {
        let room = PipelineReplyRoom {
            reply: &sized,
            capacity: READ_BUDGET,
        };
        if room
            .check_reply(&slice_pipeline(fitting + 1, &catalogue).largest())
            .is_err()
        {
            break;
        }
        fitting += 1;
    }
    fitting
}

#[test]
fn draft_save_reply_has_measured_room_for_pipeline_advice_beside_sizing() {
    let reply = candidate_mutation(saved()).unwrap();
    let base = responses::encoded_len(&reply).unwrap();
    // A reply that holds no `advice` field, or a short one, leaves the most room.
    let (alone, each) = most_pipeline_nodes(&reply);
    let one = largest_pipeline_bytes(&reply, 1);
    // A first draft is asked about whole by both points; each field at its largest.
    let (bare, usual, heavy) = (
        most_nodes_with_both(&reply, 0),
        most_nodes_with_both(&reply, 5),
        most_nodes_with_both(&reply, 100),
    );
    println!(
        "slice.candidates.save base_envelope_bytes={base}; pipeline advice bytes for one asked \
         node={one}, bytes_per_further_node={each}; asked nodes that fit without a sizing \
         field={alone}; nodes of a first draft that fit beside the largest sizing field with \
         0/5/100 included items each={bare}/{usual}/{heavy}"
    );
    assert_eq!((base, one, each), (1373, 557, 134));
    assert_eq!((alone, bare, usual, heavy), (47, 22, 19, 5));
}

/// What the largest `pipeline_advice` field for this many asked nodes adds to a reply
/// that has room for it.
fn largest_pipeline_bytes(reply: &Value, nodes: usize) -> usize {
    let largest = slice_pipeline(nodes, &served_catalogue()).largest();
    let with = finish_pipeline(reply.clone(), Some(largest), READ_BUDGET).unwrap();
    responses::encoded_len(&with).unwrap() - responses::encoded_len(reply).unwrap()
}

#[test]
fn the_largest_sizing_field_can_leave_less_room_than_any_pipeline_field_needs() {
    let reply = candidate_mutation(saved()).unwrap();
    // The most candidates with five included items each that `sizing` advises on,
    // every answer of Jev at its longest: what is left of the read budget.
    let (most, _) = most_candidates(&reply, slice_sizing, 5);
    let sized = with_full_sizing(&reply, most, 5).unwrap();
    let left = READ_BUDGET - responses::encoded_len(&sized).unwrap();
    // The shortest field the point then has: it says its advice would not fit.
    let unfit = AdviceField::unavailable(
        AdviceUnavailableReason::AdviceTooLarge,
        &format!(
            "advice for {} would not fit the command reply",
            slice_pipeline(1, &served_catalogue()).subject()
        ),
    );
    let need = largest_bytes_of(&reply, &unfit);
    println!(
        "slice.candidates.save with the largest sizing field for {most} candidates of 5 \
         included items: bytes_left={left}; the advice_too_large pipeline field needs={need}"
    );
    assert_eq!((most, left, need), (37, 148, 225));
    // With less left than that, the reply cannot carry the field of the second point.
    assert_eq!(
        finish_pipeline(sized, Some(unfit), READ_BUDGET),
        Err(Error::RequestTooLarge)
    );
}

/// What a field adds under `pipeline_advice` to a reply that has room for it.
fn largest_bytes_of(reply: &Value, field: &AdviceField) -> usize {
    let with = finish_pipeline(reply.clone(), Some(field.clone()), READ_BUDGET).unwrap();
    responses::encoded_len(&with).unwrap() - responses::encoded_len(reply).unwrap()
}
