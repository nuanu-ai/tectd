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
            .and_then(|page| scope_candidate_output::page(page, after.unwrap_or(0), capacity)),
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
                scope_candidate_output::fragment(
                    candidate_set_id,
                    draft_revision,
                    fragment,
                    capacity,
                )
            }),
        ScopeCandidateInvocation::Begin(request) => service
            .begin_candidate_set(context, &request, &guidance, &guard)
            .await
            .and_then(|outcome| scope_candidate_output::begin(outcome, capacity)),
        ScopeCandidateInvocation::SaveDraft(request) => service
            .save_candidate_draft(context, &request, &guidance, &guard)
            .await
            .and_then(|stored| scope_candidate_output::stored(stored, capacity)),
        ScopeCandidateInvocation::Review(request) => service
            .review_candidate_set(context, &request, &guidance, &guard)
            .await
            .and_then(|stored| scope_candidate_output::stored(stored, capacity)),
        ScopeCandidateInvocation::RecordInput(request) => service
            .record_candidate_input(context, &request, &guard)
            .await
            .and_then(|stored| scope_candidate_output::stored(stored, capacity)),
        ScopeCandidateInvocation::Refresh(request) => service
            .refresh_candidate_set(context, &request, &guidance, &guard)
            .await
            .and_then(|stored| scope_candidate_output::stored(stored, capacity)),
    }
}
