use crate::scope_candidate_tools::ScopeCandidateInvocation;
use crate::scope_guidance::{CandidateEncoding, StaticCandidateGuidance};
use crate::{Result, scope_candidate_output};
use tect_application::WorkspaceService;
use tect_domain::{CandidateContextQuery, RequestContext};

const FRAGMENT_BYTES: usize = 256 * 1024;

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: ScopeCandidateInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<serde_json::Value> {
    let guidance = StaticCandidateGuidance;
    let guard = CandidateEncoding { capacity };
    match invocation {
        ScopeCandidateInvocation::Window { request, window, params, candidate_set_revision } => match *request {
            ScopeCandidateInvocation::Context { candidate_set_id, view, draft_revision, after, limit } => service.candidate_context(context, &CandidateContextQuery { candidate_set_id, view, draft_revision, after, limit }, &guidance).await.and_then(|page| scope_candidate_output::page_read(page, params, &window, candidate_set_revision, capacity)),
            ScopeCandidateInvocation::Fragment { candidate_set_id, draft_revision, source_ref_id, cursor } => service.candidate_fragment(context, candidate_set_id, draft_revision, source_ref_id, cursor, FRAGMENT_BYTES).await.and_then(|fragment| scope_candidate_output::fragment_read(candidate_set_id, draft_revision, fragment, params, &window, capacity)),
            _ => Err(tect_domain::Error::InternalInvariant),
        },
        ScopeCandidateInvocation::Context {
            candidate_set_id,
            view,
            draft_revision,
            after,
            limit,
        } => service
            .candidate_context(
                context,
                &CandidateContextQuery {
                    candidate_set_id,
                    view,
                    draft_revision,
                    after,
                    limit,
                },
                &guidance,
            )
            .await
            .and_then(|page| {
                let mut params = serde_json::json!({"candidate_set_id":candidate_set_id,"view":view,"after":after.unwrap_or(0),"limit":limit});
                if let Some(revision) = draft_revision { params["draft_revision"] = serde_json::json!(revision); }
                scope_candidate_output::page_read(page, params, &crate::planning_read::Window::default(), None, capacity)
            }),
        ScopeCandidateInvocation::Fragment {
            candidate_set_id,
            draft_revision,
            source_ref_id,
            cursor,
        } => service
            .candidate_fragment(
                context,
                candidate_set_id,
                draft_revision,
                source_ref_id,
                cursor,
                FRAGMENT_BYTES,
            )
            .await
            .and_then(|fragment| {
                {
                    let mut params = serde_json::json!({"candidate_set_id":candidate_set_id,"view":"fragment","source_ref_id":source_ref_id,"cursor":cursor});
                    if let Some(revision) = draft_revision { params["draft_revision"] = serde_json::json!(revision); }
                    scope_candidate_output::fragment_read(candidate_set_id, draft_revision, fragment, params, &crate::planning_read::Window::default(), capacity)
                }
            }),
        ScopeCandidateInvocation::Begin(request) => service
            .begin_candidate_set(context, &request, &guidance, &guard)
            .await
            .and_then(|outcome| scope_candidate_output::begin(outcome, capacity)),
        ScopeCandidateInvocation::SaveDraft(request) => service
            .save_candidate_draft(context, &request, &guidance, &guard)
            .await
            .and_then(|stored| scope_candidate_output::stored_mutation(stored, capacity)),
        ScopeCandidateInvocation::Review(request) => service
            .review_candidate_set(context, &request, &guidance, &guard)
            .await
            .and_then(|stored| scope_candidate_output::stored_mutation(stored, capacity)),
        ScopeCandidateInvocation::RecordInput(request) => service
            .record_candidate_input(context, &request, &guard)
            .await
            .and_then(|stored| scope_candidate_output::stored_mutation(stored, capacity)),
        ScopeCandidateInvocation::Refresh(request) => service
            .refresh_candidate_set(context, &request, &guidance, &guard)
            .await
            .and_then(|stored| scope_candidate_output::stored(stored, capacity)),
        ScopeCandidateInvocation::DeltaApply(request) => service
            .apply_candidate_delta(context, &request)
            .await
            .and_then(|receipt| scope_candidate_output::delta(receipt, capacity)),
        ScopeCandidateInvocation::DeltaStatus {
            candidate_set_id,
            idempotency_key,
        } => service
            .candidate_delta_status(context, candidate_set_id, &idempotency_key)
            .await
            .and_then(|receipt| scope_candidate_output::delta(receipt, capacity)),
    }
}
