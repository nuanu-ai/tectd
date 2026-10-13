//! How much advice the replies of the Scope candidate commands can carry: `sizing` on
//! a draft save, `intake` and beside it `model` on a recorded input. Measured with the
//! real reply and the real encoder.
use super::*;
use crate::advice_reply::model_tests::{InputRoom, input_room, reserve_holds};
use crate::advice_reply::tests::{largest_bytes, most_candidates, scope_intake, scope_sizing};

/// The reply of a command that left the set waiting for review, with these stale
/// reasons. The method identity is as long as the real one, since the reply carries it.
fn reply(stale_reasons: &[&str]) -> Value {
    let mut context = super::planning_read_tests::fixture().context;
    let id = context.candidate_set.id;
    context.candidate_set.status = CandidateSetStatus::ReviewRequired;
    context.stale_reasons = stale_reasons
        .iter()
        .map(|reason| (*reason).into())
        .collect();
    context.snapshot.method.id = crate::scope_guidance::METHOD_ID.into();
    context.snapshot.method.digest = "0".repeat(64);
    let stored = StoredCandidateContext {
        context,
        program: tect_domain::Program::draft(id, id, 0),
        draft: None,
        reviews: vec![],
    };
    stored_mutation(stored, 8192).unwrap()
}

#[test]
fn draft_save_reply_has_measured_room_for_sizing_advice() {
    // A stored draft always leaves the set waiting for review.
    let reply = reply(&[]);
    let base = encoded_len(&reply).unwrap();
    let (bare, each) = most_candidates(&reply, scope_sizing, 0);
    let (usual, _) = most_candidates(&reply, scope_sizing, 5);
    let (heavy, _) = most_candidates(&reply, scope_sizing, 100);
    println!(
        "scope.candidates.save base_envelope_bytes={base}; sizing advice bytes_per_candidate={each}; \
         candidates_that_fit with 0/5/100 included items each={bare}/{usual}/{heavy}"
    );
    assert_eq!((base, each), (2954, 133));
    assert_eq!((bare, usual, heavy), (35, 28, 5));
}

#[test]
fn record_input_reply_has_measured_room_for_intake_advice() {
    // A recorded input leaves the snapshot stale until a refresh.
    let reply = reply(&["planning_inputs"]);
    let base = encoded_len(&reply).unwrap();
    let bare = largest_bytes(&reply, &scope_intake(None));
    let named = largest_bytes(&reply, &scope_intake(Some(120)));
    println!(
        "scope.candidates.record_input base_envelope_bytes={base}; largest intake advice bytes \
         without a saved draft={bare}, with one={named}"
    );
    assert_eq!((base, bare, named), (3016, 888, 931));
}

#[test]
fn record_input_reply_has_measured_room_for_model_advice_beside_intake() {
    let reply = reply(&["planning_inputs"]);
    let room = input_room(&reply, &scope_intake(Some(120)));
    println!(
        "scope.candidates.record_input base_envelope_bytes={}; largest model advice bytes={}; \
         its advice_too_large field bytes={}; bytes_left beside the largest intake field and \
         the largest model field={}",
        room.base, room.largest, room.short, room.left
    );
    assert_eq!(
        room,
        InputRoom {
            base: 3016,
            largest: 819,
            short: 194,
            left: 3426
        }
    );
}

#[test]
fn intake_measured_beside_the_reserve_leaves_the_second_point_its_short_field() {
    let reply = reply(&["planning_inputs"]);
    let bare = reserve_holds(&reply, &scope_intake(None));
    let named = reserve_holds(&reply, &scope_intake(Some(120)));
    println!(
        "scope.candidates.record_input: the largest intake field and the reserve it is \
         measured beside, in bytes, without a saved draft={bare:?}, with one={named:?}"
    );
    assert_eq!((bare, named), ((888, 194), (931, 194)));
}
