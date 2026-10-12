//! How much `sizing` advice the Slice candidate draft-save reply can carry, measured
//! with the real reply and the real encoder.
use super::*;
use crate::advice_reply::tests::{most_candidates, slice_sizing};

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
    assert_eq!((base, each), (1373, 124));
    assert_eq!((bare, usual, heavy), (52, 48, 15));
}
