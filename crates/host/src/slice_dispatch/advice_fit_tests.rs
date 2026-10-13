//! How much advice the replies of the Slice candidate commands can carry: `sizing` and
//! beside it `pipeline` on a draft save, `intake` and beside it `model` on a recorded
//! input. Measured with the real reply and the real encoder.
use super::*;
use crate::advice_reply::model_tests::{
    InputRoom, input_room, reserve_holds, two_short_fields_need_room_for_both,
};
use crate::advice_reply::tests::{
    largest_bytes, most_candidates, most_pipeline_nodes, served_catalogue, slice_intake,
    slice_pipeline, slice_pipeline_draft, slice_sizing,
};
use crate::advice_reply::{
    PipelineReplyRoom, ReplyRoom, finish, finish_pipeline, with_pipeline_reserve,
};
use crate::json_fragment::READ_BUDGET;
use tect_application::AdviceOutputGuard;
use tect_domain::{AdviceField, AdviceShaping, Error};

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
    assert_eq!((bare, usual, heavy), (45, 36, 6));
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
    assert_eq!((base, bare, named), (1373, 886, 929));
}

#[test]
fn record_input_reply_has_measured_room_for_model_advice_beside_intake() {
    let reply = candidate_mutation(saved()).unwrap();
    let room = input_room(&reply, &slice_intake(Some(120)));
    println!(
        "slice.candidates.input base_envelope_bytes={}; largest model advice bytes={}; its \
         advice_too_large field bytes={}; bytes_left beside the largest intake field and the \
         largest model field={}",
        room.base, room.largest, room.short, room.left
    );
    assert_eq!(
        room,
        InputRoom {
            base: 1373,
            largest: 819,
            short: 194,
            left: 5071
        }
    );
}

#[test]
fn intake_measured_beside_the_reserve_leaves_the_second_point_its_short_field() {
    let reply = candidate_mutation(saved()).unwrap();
    let bare = reserve_holds(&reply, &slice_intake(None));
    let named = reserve_holds(&reply, &slice_intake(Some(120)));
    println!(
        "slice.candidates.input: the largest intake field and the reserve it is measured \
         beside, in bytes, without a saved draft={bare:?}, with one={named:?}"
    );
    assert_eq!((bare, named), ((886, 194), (929, 194)));
}

#[test]
fn a_reply_with_room_for_one_short_field_and_not_for_two_answers_with_an_error() {
    let reply = candidate_mutation(saved()).unwrap();
    let short = two_short_fields_need_room_for_both(&reply, &slice_intake(Some(120)));
    println!(
        "slice.candidates.input: the advice_too_large fields of intake and of model, in \
         bytes={short:?}; a reply with less than their sum left answers with an error"
    );
    assert_eq!(short, (171, 194));
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
    assert_eq!((base, one, each), (1373, 740, 134));
    assert_eq!((alone, bare, usual, heavy), (46, 20, 18, 5));
}

/// What the largest `pipeline_advice` field for this many asked nodes adds to a reply
/// that has room for it.
fn largest_pipeline_bytes(reply: &Value, nodes: usize) -> usize {
    let largest = slice_pipeline(nodes, &served_catalogue()).largest();
    let with = finish_pipeline(reply.clone(), Some(largest), READ_BUDGET).unwrap();
    responses::encoded_len(&with).unwrap() - responses::encoded_len(reply).unwrap()
}

#[test]
fn sizing_measured_beside_the_reserve_leaves_the_second_point_its_short_field() {
    let reply = candidate_mutation(saved()).unwrap();
    let catalogue = served_catalogue();
    for items in [0, 5, 100] {
        // Without the reserve the largest `sizing` field can fill the reply to within
        // less than the short field the second point needs to say it has no room.
        let (unreserved, _) = most_candidates(&reply, slice_sizing, items);
        // With it: the most candidates of a first draft that `sizing` advises on when
        // it is measured on the reply that holds the reserve for the same draft.
        let mut most = 0;
        loop {
            let unfit = slice_pipeline_draft(most + 1, &catalogue).without_room();
            let room = ReplyRoom {
                reply: &with_pipeline_reserve(&reply, &unfit),
                capacity: READ_BUDGET,
            };
            if room
                .check_reply(&slice_sizing(most + 1, items).largest())
                .is_err()
            {
                break;
            }
            most += 1;
        }
        // The reserve is 226 bytes and a candidate costs 139 or more: it takes the
        // room of two candidates at most.
        assert!(most <= unreserved && unreserved <= most + 2, "{items}");
        // Whatever `sizing` then gives, the reply carries the second point's field.
        let unfit = slice_pipeline_draft(most, &catalogue).without_room();
        let need = largest_bytes_of(&reply, &unfit);
        let sized = with_full_sizing(&reply, most, items).unwrap();
        let left = READ_BUDGET - responses::encoded_len(&sized).unwrap();
        println!(
            "slice.candidates.save, first draft of candidates with {items} included items: \
             sizing advises on {unreserved} without the reserve and on {most} with it; \
             bytes_left={left}; the advice_too_large pipeline field needs={need}"
        );
        assert!(left >= need, "{items}: {left} < {need}");
        finish_pipeline(sized, Some(unfit), READ_BUDGET).unwrap();
    }
    // The case the reserve is for: the largest `sizing` field for thirty-six
    // candidates of five items fits the reply and leaves less than the 226 bytes the
    // second point needs. Measured with the reserve, `sizing` gives way at thirty-six.
    let unfit = slice_pipeline_draft(36, &catalogue).without_room();
    assert_eq!(largest_bytes_of(&reply, &unfit), 226);
    let sized = with_full_sizing(&reply, 36, 5).unwrap();
    let left = READ_BUDGET - responses::encoded_len(&sized).unwrap();
    println!(
        "slice.candidates.save, 36 candidates of 5 items without the reserve: bytes_left={left}"
    );
    assert_eq!(left, 122);
    let room = ReplyRoom {
        reply: &with_pipeline_reserve(&reply, &unfit),
        capacity: READ_BUDGET,
    };
    assert_eq!(
        room.check_reply(&slice_sizing(36, 5).largest()),
        Err(Error::RequestTooLarge)
    );
    assert_eq!(room.check_reply(&slice_sizing(35, 5).largest()), Ok(()));
}

/// What a field adds under `pipeline_advice` to a reply that has room for it.
fn largest_bytes_of(reply: &Value, field: &AdviceField) -> usize {
    let with = finish_pipeline(reply.clone(), Some(field.clone()), READ_BUDGET).unwrap();
    responses::encoded_len(&with).unwrap() - responses::encoded_len(reply).unwrap()
}
