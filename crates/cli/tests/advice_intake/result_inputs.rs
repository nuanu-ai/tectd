//! A Slice result writes an input entry of its own. Another command writes it, so it
//! does not fire the point; and it is not the user's words, so the next input does
//! not carry it to Jev.
use crate::drafts::{Level, Shape};
use crate::inputs::{FIRST, Planned, route};
use crate::planning::{open_slice, review, route as call};
use crate::recovery_support::candidate_reads::read_query_json;
use crate::recovery_support::native_reads::SlicePlanningFixture;
use crate::support::{Jev, Stand, command, configure};
use serde_json::json;
use tect_domain::ProgramMode;
use uuid::Uuid;

fn settled(name: &str) -> (&'static str, f64) {
    match name {
        "work" => ("small", 0.7),
        "placement" => ("covered", 0.7),
        "nearest" => ("c1", 0.7),
        _ => ("in_line", 0.7),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_input_a_slice_result_produced_neither_fires_the_point_nor_counts_as_user_words() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;

    // One work node, saved and reviewed ready before Jev is enabled.
    let mut planned = Planned::bare(&mut client, &repo, Level::Slices).await;
    let save = planned.target.save(&Shape::of(1, 1), Uuid::new_v4());
    let (saved, _) = command(&mut client, Level::Slices.save_route(), save).await;
    let saved = SlicePlanningFixture::from_mutation(saved)
        .read_details(&mut client)
        .await
        .value;
    let ready = review(&mut client, &saved).await;
    assert_eq!(ready["candidate_set"]["status"], "ready");
    configure(&mut client, true, 0).await;

    // The Slice is opened and its result recorded: an input entry appears that the
    // user did not write.
    let node = ready["draft"]["nodes"][0].clone();
    let opened = call(
        &mut client,
        "command",
        "slice.open",
        open_slice(&ready, &node, Uuid::new_v4()),
    )
    .await;
    let slice = &opened["created"];
    let summary = "The preview reads a stale settings cache";
    call(
        &mut client,
        "command",
        "slice.result.record",
        json!({"request_id":Uuid::new_v4(),"scope_id":ready["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],"outcome":"completed",
            "summary":summary,"evidence":[{"kind":"test","reference":"fixture",
                "observation":"Result persisted"}],
            "scope_impact":"The correction can be chosen","remaining_work":"Choose it"}),
    )
    .await;
    assert_eq!(stand.count("advice_journal").await, 0);
    assert_eq!(stand.jev.calls(), 0);
    let details = read_query_json(
        &mut client,
        &json!({"route":"slice.candidates.context","params":{
            "scope_id":ready["scope"]["id"],"view":"details","limit":25}}),
    )
    .await
    .value;
    let inputs = details["inputs"].as_array().unwrap();
    assert_eq!(inputs.len(), 2);
    assert!(inputs[0]["source_result_id"].is_null());
    assert!(inputs[1]["source_result_id"].is_string());
    assert!(inputs[1]["input"].as_str().unwrap().ends_with(summary));

    // The user's next input fires the point. Jev reads the user's words and not the
    // entry the result produced, which stands between them in the store.
    planned.follow(&details);
    let params = planned.input(FIRST, Uuid::new_v4());
    let (recorded, _) = command(&mut client, route(Level::Slices), params).await;
    assert_eq!(recorded["advice"]["status"], "advised");
    assert_eq!(recorded["advice"]["nearest"], 1);
    let requests = stand.jev.requests();
    assert_eq!(requests.len(), 1);
    let shown = json!([{"key":"c1","kind":"work","title":"Work 1",
        "outcome":"The cause is demonstrated","includes":[{"key":"c1.i1","text":"item 1"}],
        "excludes":["deployment"],"dependencies":[]}]);
    assert_eq!(
        requests[0]["state"],
        planned.state(
            ProgramMode::Prod,
            &planned.first_words(),
            FIRST,
            Some(shown)
        )
    );
    assert!(!requests[0].to_string().contains(summary));
    assert_eq!(planned.recorded(&stand).await.len(), 3);
    assert_eq!(stand.count("advice_journal").await, 1);
    assert_eq!(stand.newest_row().await["point"], "intake");
    client.finish().await;
}
