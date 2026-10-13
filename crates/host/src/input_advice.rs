//! The two advice points of a recorded input, asked in order on the reply the command
//! built: `intake` gives `advice`, then `model` gives `model_advice`.
use crate::advice_reply::{self, ModelReplyRoom, ReplyRoom};
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{
    ModelAdvice, RecordCandidateInput, RecordSliceCandidateInput, RequestContext, Result,
    SliceCandidateContext, StoredCandidateContext,
};

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
