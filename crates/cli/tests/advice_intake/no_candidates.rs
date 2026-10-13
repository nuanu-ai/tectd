//! Input recorded for a set that offers no candidate to name: no saved draft at all,
//! or a saved draft that holds decision nodes only.
use crate::drafts::Level;
use crate::inputs::{FIRST, Planned, asked, criteria, placement_without_candidates, route};
use crate::support::{Jev, Stand, command, configure};
use serde_json::json;
use tect_domain::{INTAKE_NOTE, ProgramMode};
use uuid::Uuid;

/// Large new work that needs a candidate of its own and asks for more than the level needs.
fn growing(name: &str) -> (&'static str, f64) {
    match name {
        "work" => ("large", 0.7),
        "placement" => ("new_candidate", 0.6),
        "fit" => ("beyond_mode", 0.8),
        // The `model` point asks about the same input; this suite is about `intake`.
        "practical" | "specialist" => ("ABSTAIN", 0.9),
        other => panic!("unexpected question {other}"),
    }
}

/// The field for `growing`: there is no `nearest`, since the question was not asked.
fn grown(level: Level) -> serde_json::Value {
    json!({"status":"advised","note":INTAKE_NOTE,"level":level.name(),
        "work":"large","work_p":0.7,"placement":"new_candidate","placement_p":0.6,
        "fit":"beyond_mode","fit_p":0.8})
}

async fn input_without_a_saved_draft_gets_no_nearest_question(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(growing));
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::bare(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    let params = planned.input(FIRST, Uuid::new_v4());
    let (recorded, _) = command(&mut client, route(level), params).await;
    assert_eq!(recorded["advice"], grown(level));

    let requests = stand.jev.asked("fit");
    assert_eq!(requests.len(), 1);
    let state = &requests[0]["state"];
    assert!(state.get("candidates").is_none(), "{state}");
    assert_eq!(
        *state,
        planned.state(ProgramMode::Prod, &planned.first_words(), FIRST, None)
    );
    assert_eq!(asked(&requests[0]), ["fit", "placement", "work"]);
    assert_eq!(
        criteria(&requests[0], "placement"),
        placement_without_candidates(level)
    );
    let row = stand.newest_row_of("intake").await;
    assert_eq!(row["advice"], recorded["advice"]);
    assert_eq!(row["advice"].to_string(), recorded["advice"].to_string());
    assert_eq!(
        (&row["point"], &row["outcome"]),
        (&json!("intake"), &json!("advised"))
    );
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_without_a_saved_draft_gets_no_nearest_question() {
    input_without_a_saved_draft_gets_no_nearest_question(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_without_a_saved_draft_gets_no_nearest_question() {
    input_without_a_saved_draft_gets_no_nearest_question(Level::Slices).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_for_a_draft_of_decision_nodes_only_gets_no_nearest_question() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(growing));
    let (mut client, repo) = stand.workspace().await;
    let mut planned = Planned::bare(&mut client, &repo, Level::Slices).await;
    let draft = json!({"coverage_summary":"A decision comes first","supersessions":[],
        "nodes":[{"kind":"decision","identity":{"local":"decision"},
            "title":"Choose correction path","question":"Is Lightweight sufficient?",
            "resolution_criteria":["Diagnosis identifies the boundary"],
            "dependencies":[],"source_result_ids":[]}]});
    let save = planned.target.save_draft(draft, Uuid::new_v4());
    planned.save(&mut client, save).await;
    configure(&mut client, true, 0).await;
    let params = planned.input(FIRST, Uuid::new_v4());
    let (recorded, _) = command(&mut client, route(Level::Slices), params).await;
    assert_eq!(recorded["advice"], grown(Level::Slices));

    // The decision node is shown to Jev, and there is still nothing to name.
    let requests = stand.jev.asked("fit");
    let shown = json!([{"key":"c1","kind":"decision","title":"Choose correction path",
        "question":"Is Lightweight sufficient?","dependencies":[]}]);
    assert_eq!(
        requests[0]["state"],
        planned.state(
            ProgramMode::Prod,
            &planned.first_words(),
            FIRST,
            Some(shown)
        )
    );
    assert_eq!(asked(&requests[0]), ["fit", "placement", "work"]);
    assert_eq!(
        criteria(&requests[0], "placement"),
        placement_without_candidates(Level::Slices)
    );
    assert_eq!(
        stand.newest_row_of("intake").await["advice"],
        recorded["advice"]
    );
    client.finish().await;
}
