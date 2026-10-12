//! A candidate with a mode override of its own carries its level to Jev in its
//! object, and inside an opened Scope the request's level is the Scope's.
use crate::advised::walk;
use crate::support::Stand;
use serde_json::{Value, json};
use tect_domain::ProgramMode;

fn level(mode: ProgramMode) -> Value {
    json!({"level":mode.as_str(),"description":mode.description()})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn sizing_reads_a_candidates_own_level_and_the_scopes_level_inside_a_scope() {
    let stand = Stand::start("2000").await;
    let sent = walk(&stand).await;

    // Program to Scopes: the level is the Program's; the permitted candidate has its own.
    let state = &sent.scope_save["state"];
    assert_eq!(state["level"], "program_to_scopes");
    assert_eq!(state["mode"], level(ProgramMode::Prod));
    assert_eq!(
        state["candidates"][0]["mode_override"],
        level(ProgramMode::Demo)
    );
    assert!(state["candidates"][1].get("mode_override").is_none());
    assert!(
        sent.scope_save["questions"]["c1.size"]["instructions"]
            .as_str()
            .unwrap()
            .contains("its own mode_override when it carries one, else state.mode")
    );

    // Scope to Slices: the Scope was opened from the candidate permitted demo, so
    // demo is the level here although the Program is prod; one node has its own.
    let state = &sent.slice_save["state"];
    assert_eq!(state["level"], "scope_to_slices");
    assert_eq!(state["mode"], level(ProgramMode::Demo));
    assert_eq!(
        state["candidates"][0]["mode_override"],
        level(ProgramMode::Mvp)
    );
    assert!(state["candidates"][1].get("mode_override").is_none());

    // Jev is sent levels and their meaning: no id of an override and no key of one.
    for request in [&sent.scope_save, &sent.slice_save] {
        let text = request.to_string();
        assert!(
            !text.contains("source_ref_id") && !text.contains("input_id"),
            "{text}"
        );
        assert!(!text.contains("exact_quote"), "{text}");
    }
    assert_eq!(stand.count("advice_journal").await, 5);
}
