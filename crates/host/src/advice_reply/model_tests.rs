//! The `model_advice` field beside the `advice` field of a recorded input, and what the
//! reply tests of both input commands measure with: the real encoder, nothing computed.
use super::tests::scope_intake;
use super::*;
use crate::json_fragment::READ_BUDGET;
use serde_json::json;
use tect_domain::{
    AdviceShaping, AdviceUnavailableReason, IntakeAdvice, IntakePlanning, ModelAdvice, Program,
    ProgramMode,
};
use uuid::Uuid;

/// The `model` point for a recorded input. What the input lands on changes what Jev
/// reads and nothing of the field.
pub(crate) fn model() -> ModelAdvice {
    let id = Uuid::new_v4();
    let planning = IntakePlanning::scope_candidates(None).unwrap();
    let program = Program::draft(id, id, 0);
    ModelAdvice::of(&planning, &program, ProgramMode::Prod, &[], "i")
}

/// What a field adds under `model_advice` to a reply that has room for it.
pub(crate) fn model_bytes(reply: &Value, field: &AdviceField) -> usize {
    let room = ModelReplyRoom {
        reply,
        capacity: READ_BUDGET,
    };
    assert_eq!(room.check_reply(field), Ok(()));
    let with = finish_model(reply.clone(), Some(field.clone()), READ_BUDGET).unwrap();
    responses::encoded_len(&with).unwrap() - responses::encoded_len(reply).unwrap()
}

/// What the room of an input reply measures to, in bytes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InputRoom {
    /// The reply as the command builds it.
    pub base: usize,
    /// The largest `model_advice` field.
    pub largest: usize,
    /// The field the point gives without room: what `intake` is measured beside.
    pub short: usize,
    /// What the read budget has left when both points gave their largest field.
    pub left: usize,
}

/// The room of an input reply for the second point, with `intake` at its largest.
pub(crate) fn input_room(reply: &Value, intake: &IntakeAdvice) -> InputRoom {
    let base = responses::encoded_len(reply).unwrap();
    let point = model();
    let largest = model_bytes(reply, &point.largest());
    let short = model_bytes(reply, &ModelAdvice::without_room());
    // The second field costs the same beside the first, and both fit.
    let advised = finish(reply.clone(), Some(intake.largest()), READ_BUDGET).unwrap();
    assert_eq!(model_bytes(&advised, &point.largest()), largest);
    let both = finish_model(advised, Some(point.largest()), READ_BUDGET).unwrap();
    InputRoom {
        base,
        largest,
        short,
        left: READ_BUDGET - responses::encoded_len(&both).unwrap(),
    }
}

/// What the reserve does at the capacities where it matters: from a reply that the
/// largest `intake` field fills exactly, up to one that also holds the second point's
/// short field. Returns the bytes of that largest field and of the short one.
pub(crate) fn reserve_holds(reply: &Value, intake: &IntakeAdvice) -> (usize, usize) {
    let base = responses::encoded_len(reply).unwrap();
    let short = ModelAdvice::without_room();
    let measured = with_model_reserve(reply, &short);
    let advised = finish(reply.clone(), Some(intake.largest()), READ_BUDGET).unwrap();
    let first = responses::encoded_len(&advised).unwrap() - base;
    let second = model_bytes(reply, &short);
    assert_eq!(responses::encoded_len(&measured).unwrap(), base + second);
    for capacity in base + first..=base + first + second {
        let enough = capacity == base + first + second;
        // Measured on the reply as it is, `intake` is asked at every one of these
        // capacities, and at all but the last its largest field leaves the second
        // point less than its short field needs: the stored input would answer with
        // an error.
        let unreserved = ReplyRoom { reply, capacity };
        assert_eq!(unreserved.check_reply(&intake.largest()), Ok(()));
        let filled = finish(reply.clone(), Some(intake.largest()), capacity).unwrap();
        let answered = finish_model(filled, Some(short.clone()), capacity);
        assert_eq!(answered.is_ok(), enough, "{capacity}");
        // Measured beside the reserve, `intake` is asked only where the short field
        // still fits beside its largest one.
        let reserved = ReplyRoom {
            reply: &measured,
            capacity,
        };
        assert_eq!(
            reserved.check_reply(&intake.largest()).is_ok(),
            enough,
            "{capacity}"
        );
    }
    // One byte short of that, `intake` is the one that is not asked. Its own short
    // field leaves the second point room for its largest advice.
    let capacity = base + first + second - 1;
    let unasked = AdviceField::too_large(&intake.subject());
    let given_way = finish(reply.clone(), Some(unasked), capacity).unwrap();
    let room = ModelReplyRoom {
        reply: &given_way,
        capacity,
    };
    assert_eq!(room.check_reply(&model().largest()), Ok(()));
    (first, second)
}

fn reply() -> Value {
    responses::with_actions(
        json!({"operation":"saved","counts":{"candidates":2}}),
        vec![],
        None,
    )
}

#[test]
fn the_model_field_is_attached_under_its_own_key_beside_advice() {
    let intake = scope_intake(Some(2)).largest();
    let reply = finish(reply(), Some(intake.clone()), READ_BUDGET).unwrap();
    let bytes = serde_json::to_vec(&reply).unwrap();
    // No field from the point: the reply that holds `advice` leaves as it is.
    for capacity in [READ_BUDGET, 1] {
        let finished = finish_model(reply.clone(), None, capacity).unwrap();
        assert_eq!(serde_json::to_vec(&finished).unwrap(), bytes);
    }

    let advice = model().largest();
    let finished = finish_model(reply.clone(), Some(advice.clone()), READ_BUDGET).unwrap();
    assert_eq!(finished["model_advice"], json!(advice));
    assert_eq!(finished["advice"], json!(intake));
    let mut without = finished.clone();
    without.as_object_mut().unwrap().remove("model_advice");
    assert_eq!(serde_json::to_vec(&without).unwrap(), bytes);

    // The room is what the reply has left with the `advice` field it holds: the guard
    // and the reply that leaves agree at every capacity.
    let exact = responses::encoded_len(&finished).unwrap();
    for (capacity, fits) in [(exact, true), (exact - 1, false), (usize::MAX, true)] {
        let room = ModelReplyRoom {
            reply: &reply,
            capacity,
        };
        assert_eq!(room.check_reply(&advice).is_ok(), fits, "{capacity}");
        assert_eq!(
            finish_model(reply.clone(), Some(advice.clone()), capacity).is_ok(),
            fits
        );
    }

    // Any field goes through the same check, not only an advised one.
    let unavailable = AdviceField::unavailable(AdviceUnavailableReason::Timeout, "late");
    let finished = finish_model(reply.clone(), Some(unavailable), READ_BUDGET).unwrap();
    assert_eq!(finished["model_advice"]["reason"], "timeout");
    assert_eq!(finished["advice"], json!(intake));
    assert_eq!(
        finish_model(
            json!("not an object"),
            Some(AdviceField::Abstained),
            READ_BUDGET
        ),
        Err(Error::InternalInvariant)
    );
}

#[test]
fn the_reserve_is_measured_and_never_leaves_with_the_reply() {
    let reply = reply();
    let bytes = serde_json::to_vec(&reply).unwrap();
    let short = ModelAdvice::without_room();
    let measured = with_model_reserve(&reply, &short);
    assert_eq!(measured["model_advice"], json!(short));
    // The reply it was copied from is the one that leaves, untouched.
    assert_eq!(serde_json::to_vec(&reply).unwrap(), bytes);
    let mut without = measured.clone();
    without.as_object_mut().unwrap().remove("model_advice");
    assert_eq!(serde_json::to_vec(&without).unwrap(), bytes);
    // Nothing is checked: a reply that is not an object comes back as it is.
    assert_eq!(
        with_model_reserve(&json!("not an object"), &short),
        json!("not an object")
    );
}
