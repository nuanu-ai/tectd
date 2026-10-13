//! Drafts as a save stored them, for the tests of the points that read one: as far
//! as an advice point reads a stored draft, it is the id and the revision of each
//! candidate, in draft order.
use crate::{ResolvedCandidateDraft, ResolvedSliceCandidateDraft};
use serde_json::json;
use uuid::Uuid;

/// A stored Scope candidate draft whose candidates have these ids and revisions.
pub(super) fn stored_scopes(kept: &[(Uuid, i64)]) -> ResolvedCandidateDraft {
    let candidates = kept
        .iter()
        .map(|(id, revision)| {
            json!({"id":id,"revision":revision,"title":"t","outcome":"o","trigger":"t",
                "delivered_behavior":"b","proof":"p","includes":[],"excludes":[],
                "dependencies":[],"coverage_goal_ids":[],"evidence_ids":[]})
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({"boundary":"ongoing","goals":[],"evidence":[],
        "candidates":candidates,"blockers":[],"protected_changes":[]}))
    .unwrap()
}

/// A stored Slice candidate draft whose nodes have these ids and revisions. Which of
/// them is work and which a decision is told by the draft the save carried.
pub(super) fn stored_slices(kept: &[(Uuid, i64)]) -> ResolvedSliceCandidateDraft {
    let nodes = kept
        .iter()
        .map(|(id, revision)| {
            json!({"kind":"work","id":id,"revision":revision,"title":"t","outcome":"o",
                "includes":[],"excludes":[],"dependencies":[],"proof":["p"],
                "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r",
                "why_lightweight_insufficient":null,
                "why_further_vertical_split_not_viable":null,"source_result_ids":[]})
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({"coverage_summary":"s","nodes":nodes,"supersessions":[]}))
        .unwrap()
}
