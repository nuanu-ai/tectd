//! A recorded input is judged at the level it lands on: the Program's on the way to
//! Scopes, the opened Scope's inside it, with each saved candidate's own level in
//! its object.
use crate::advised::walk;
use crate::support::Stand;
use serde_json::{Value, json};
use tect_domain::ProgramMode;

fn level(mode: ProgramMode) -> Value {
    json!({"level":mode.as_str(),"description":mode.description()})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn intake_reads_the_scopes_level_inside_a_scope_and_each_candidates_own() {
    let stand = Stand::start("2000").await;
    let sent = walk(&stand).await;

    // Program to Scopes: the saved draft's permitted candidate carries its own level.
    let state = &sent.scope_input["state"];
    assert_eq!(state["level"], "program_to_scopes");
    assert_eq!(state["new_input"], "Also keep the audit log readable.");
    assert_eq!(state["mode"], level(ProgramMode::Prod));
    assert_eq!(
        state["candidates"][0]["mode_override"],
        level(ProgramMode::Demo)
    );
    assert!(state["candidates"][1].get("mode_override").is_none());

    // Inside the Scope opened from that candidate the level is demo, before any
    // Slice draft exists and after one is saved; the Program is still prod.
    let state = &sent.slice_first_input["state"];
    assert_eq!(state["level"], "scope_to_slices");
    assert_eq!(state["mode"], level(ProgramMode::Demo));
    assert!(state.get("candidates").is_none());
    let state = &sent.slice_input["state"];
    assert_eq!(state["new_input"], "Name the file after the day.");
    assert_eq!(state["mode"], level(ProgramMode::Demo));
    assert_eq!(
        state["candidates"][0]["mode_override"],
        level(ProgramMode::Mvp)
    );
    assert!(state["candidates"][1].get("mode_override").is_none());
    // Three inputs, each asked about by `intake` and by `model`, two draft saves, and
    // the `pipeline` point of the Slice save.
    assert_eq!(stand.count("advice_journal").await, 9);
    assert_eq!(stand.rows_of("intake").await, (3, 3));
}
