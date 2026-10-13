//! The advice points of a recorded input, asked on the reply the command built. An
//! input of a candidate set has two, in order: `intake` gives `advice`, then `model`
//! gives `model_advice`. An input of a Program has `model` alone.
use crate::advice_reply::{self, ModelReplyRoom, ReplyRoom};
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{
    ModelAdvice, RecordCandidateInput, RecordSliceCandidateInput, RequestContext, Result,
    SliceCandidateContext, StoredCandidateContext,
};
use uuid::Uuid;

/// The reply of an input recorded on a Program, given by the Program, the request id
/// and the text, with Jev's advice on its executors, if any. Untouched when Jev is
/// disabled. `intake` is not asked here: a Program has no candidate set to place the
/// input in, so the reply holds no `advice` field, nothing is reserved, and the room
/// of `model_advice` is what the reply itself leaves.
pub(crate) async fn program(
    service: &WorkspaceService,
    context: &RequestContext,
    (program_id, request_id, input): (Uuid, Uuid, &str),
    reply: Value,
    capacity: usize,
) -> Result<Value> {
    let room = ModelReplyRoom {
        reply: &reply,
        capacity,
    };
    let advice = service
        .advise_program_input_model(context, (program_id, request_id), input, &room)
        .await?;
    advice_reply::finish_model(reply, advice, capacity)
}

/// The reply of a recorded input of a Scope candidate set with Jev's advice on it, if
/// any. Untouched when Jev is disabled.
pub(crate) async fn scope_candidates(
    service: &WorkspaceService,
    context: &RequestContext,
    request: &RecordCandidateInput,
    stored: &StoredCandidateContext,
    reply: Value,
    capacity: usize,
) -> Result<Value> {
    // `intake` is measured on a reply that already holds the short field the second
    // point gives without room, so however much of the reply its advice takes, the
    // second point can still say why it gave nothing. The reply that leaves does not
    // hold the reserve.
    let measured = advice_reply::with_model_reserve(&reply, &ModelAdvice::without_room());
    let room = ReplyRoom {
        reply: &measured,
        capacity,
    };
    let advice = service
        .advise_scope_candidate_intake(context, request, stored, &room)
        .await?;
    let reply = advice_reply::finish(reply, advice, capacity)?;

    // Its room is what the reply has left with the `advice` field it now holds.
    let room = ModelReplyRoom {
        reply: &reply,
        capacity,
    };
    let advice = service
        .advise_scope_candidate_model(context, request, stored, &room)
        .await?;
    advice_reply::finish_model(reply, advice, capacity)
}

/// The same for a recorded input of the Slice candidate set of an opened Scope.
pub(crate) async fn slice_candidates(
    service: &WorkspaceService,
    context: &RequestContext,
    request: &RecordSliceCandidateInput,
    recorded: &SliceCandidateContext,
    reply: Value,
    capacity: usize,
) -> Result<Value> {
    let measured = advice_reply::with_model_reserve(&reply, &ModelAdvice::without_room());
    let room = ReplyRoom {
        reply: &measured,
        capacity,
    };
    let advice = service
        .advise_slice_candidate_intake(context, request, recorded, &room)
        .await?;
    let reply = advice_reply::finish(reply, advice, capacity)?;

    let room = ModelReplyRoom {
        reply: &reply,
        capacity,
    };
    let advice = service
        .advise_slice_candidate_model(context, request, recorded, &room)
        .await?;
    advice_reply::finish_model(reply, advice, capacity)
}
