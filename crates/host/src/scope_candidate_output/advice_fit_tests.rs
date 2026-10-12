//! How much `sizing` advice the Scope candidate draft-save reply can carry, measured
//! with the real reply and the real encoder.
use super::*;
use crate::advice_reply::tests::{most_candidates, scope_sizing};

#[test]
fn draft_save_reply_has_measured_room_for_sizing_advice() {
    let mut context = super::planning_read_tests::fixture().context;
    let id = context.candidate_set.id;
    // A stored draft always leaves the set waiting for review. The method identity is
    // as long as the real one, since the reply carries it.
    context.candidate_set.status = CandidateSetStatus::ReviewRequired;
    context.snapshot.method.id = crate::scope_guidance::METHOD_ID.into();
    context.snapshot.method.digest = "0".repeat(64);
    let stored = StoredCandidateContext {
        context,
        program: tect_domain::Program::draft(id, id, 0),
        draft: None,
        reviews: vec![],
    };
    let reply = stored_mutation(stored, 8192).unwrap();
    let base = encoded_len(&reply).unwrap();
    let (bare, each) = most_candidates(&reply, scope_sizing, 0);
    let (usual, _) = most_candidates(&reply, scope_sizing, 5);
    let (heavy, _) = most_candidates(&reply, scope_sizing, 100);
    println!(
        "scope.candidates.save base_envelope_bytes={base}; sizing advice bytes_per_candidate={each}; \
         candidates_that_fit with 0/5/100 included items each={bare}/{usual}/{heavy}"
    );
    assert_eq!((base, each), (2954, 118));
    assert_eq!((bare, usual, heavy), (41, 38, 12));
}
