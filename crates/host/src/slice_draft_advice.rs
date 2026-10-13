//! The two advice points of a Slice draft save, asked in order on the reply the save
//! built: `sizing` gives `advice`, then `pipeline` gives `pipeline_advice`.
use crate::advice_reply::{self, PipelineReplyRoom, ReplyRoom};
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{
    Error, PipelineDraft, RequestContext, Result, SaveSliceCandidateDraft, SliceCandidateContext,
};

/// The reply of a stored Slice draft save with Jev's advice on it, if any. Untouched
/// when Jev is disabled.
pub(crate) async fn advised(
    service: &WorkspaceService,
    context: &RequestContext,
    request: &SaveSliceCandidateDraft,
    saved: &SliceCandidateContext,
    reply: Value,
    capacity: usize,
) -> Result<Value> {
    // A save answers with the draft it stored. The second point is laid out first:
    // it says which work nodes the save wrote and what the point answers without room.
    let stored = saved.draft.as_ref().ok_or(Error::InternalInvariant)?;
    let catalogue = &saved.snapshot.catalogue;
    let scope = (&saved.scope).into();
    let pipeline = PipelineDraft::slice_candidates(&request.draft, stored, catalogue, scope)?;

    // `sizing` is measured on a reply that already holds that short field, so however
    // much of the reply its advice takes, the second point can still say why it gave
    // nothing. The reply that leaves does not hold the reserve.
    let measured = match &pipeline {
        Some(draft) => advice_reply::with_pipeline_reserve(&reply, &draft.without_room()),
        None => reply.clone(),
    };
    let room = ReplyRoom {
        reply: &measured,
        capacity,
    };
    let advice = service
        .advise_slice_candidate_sizing(context, request, saved, &room)
        .await?;
    let reply = advice_reply::finish(reply, advice, capacity)?;

    // The save created and changed no work node: the second point does not fire.
    let Some(draft) = pipeline else {
        return Ok(reply);
    };
    // Its room is what the reply has left with the `advice` field it now holds.
    let room = PipelineReplyRoom {
        reply: &reply,
        capacity,
    };
    let advice = service
        .advise_slice_candidate_pipeline(context, request, saved, draft, &room)
        .await?;
    advice_reply::finish_pipeline(reply, advice, capacity)
}
