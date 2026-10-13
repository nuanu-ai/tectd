//! What Jev's answers become when it abstains, and what a replay returns.
use crate::pipeline::{DEBUG, LIGHTWEIGHT, draft, planning, points, rows, save, work};
use crate::support::{Jev, Stand, Weights, command, configure};
use serde_json::json;
use tect_domain::PIPELINE_NOTE;

fn sizing(name: &str) -> Option<Weights> {
    match name {
        size if size.ends_with(".size") => Some((("bounded", 0.7), None)),
        advice if advice.ends_with(".advice") => Some((("keep", 0.7), None)),
        pipeline if pipeline.ends_with(".pipeline") => None,
        _ => Some((("requested", 0.7), None)),
    }
}

/// Jev abstains on the first work node, giving the draft's pipeline a share of its
/// own, and chooses the draft's pipeline for the second.
fn abstaining_on_one(name: &str) -> Weights {
    sizing(name).unwrap_or_else(|| match name {
        "c1.pipeline" => (("ABSTAIN", 0.55), Some((DEBUG, 0.31))),
        _ => ((LIGHTWEIGHT, 0.9), None),
    })
}

/// Jev abstains on every pipeline question and answers `sizing`.
fn abstaining_on_all(name: &str) -> Weights {
    sizing(name).unwrap_or((("ABSTAIN", 0.9), None))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn an_abstained_node_keeps_abstain_and_all_abstained_is_one_short_field() {
    let stand = Stand::start("2000").await;

    // One node abstained on: the field is advised, that node says ABSTAIN and does
    // not differ, and still carries what Jev gave the draft's pipeline.
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Weighted(abstaining_on_one));
    let nodes = vec![work(1, DEBUG, "first"), work(2, LIGHTWEIGHT, "second")];
    let (saved, _, request_id) = save(&mut client, &head, draft(nodes.clone())).await;
    assert_eq!(
        saved["pipeline_advice"],
        json!({"status":"advised","note":PIPELINE_NOTE,"candidates":[
            {"candidate":1,"pipeline":"ABSTAIN","pipeline_p":0.55,"draft_pipeline_p":0.31,
             "differs":false},
            {"candidate":2,"pipeline":LIGHTWEIGHT,"pipeline_p":0.9,"draft_pipeline_p":0.9,
             "differs":false}
        ]}),
        "{saved}"
    );
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(written[1]["outcome"], "advised");
    assert_eq!(written[1]["advice"], saved["pipeline_advice"]);

    // Every node abstained on, in another workspace: no content and no note, while
    // `sizing` is advised on the same save.
    stand.jev.answer(Jev::Weighted(abstaining_on_all));
    let (mut other, repo) = stand.workspace().await;
    let (head, _) = planning(&mut other, &repo).await;
    configure(&mut other, true, 0).await;
    let (saved, _, request_id) = save(&mut other, &head, draft(nodes)).await;
    assert_eq!(saved["pipeline_advice"], json!({"status":"abstained"}));
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(
        (&written[1]["called"], &written[1]["outcome"]),
        (&json!(true), &json!("abstained"))
    );
    assert_eq!(written[1]["advice"], saved["pipeline_advice"]);
    assert_eq!(stand.jev.calls(), 4);
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_replay_returns_both_stored_fields_without_a_call_or_a_row() {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Weighted(abstaining_on_one));
    let params = head.save(draft(vec![
        work(1, DEBUG, "first"),
        work(2, LIGHTWEIGHT, "second"),
    ]));
    let (saved, bytes) = command(&mut client, "slice.candidates.save", params.clone()).await;
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    assert_eq!(saved["pipeline_advice"]["status"], "advised", "{saved}");
    let before = (stand.jev.calls(), stand.count("advice_journal").await);
    assert_eq!(before, (2, 2));

    // Jev would now answer differently; the replay does not ask it.
    stand.jev.answer(Jev::Weighted(abstaining_on_all));
    for _ in 0..2 {
        let (replayed, replayed_bytes) =
            command(&mut client, "slice.candidates.save", params.clone()).await;
        assert_eq!(replayed, saved);
        assert_eq!(replayed_bytes, bytes);
    }
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        before
    );
    assert_eq!(stand.rows_of("pipeline").await, (1, 1));
    assert_eq!(stand.rows_of("sizing").await, (1, 1));
    client.finish().await;
}
