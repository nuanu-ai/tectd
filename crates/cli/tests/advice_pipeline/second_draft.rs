//! Later drafts: Jev is asked only about the work nodes a save created or changed.
use crate::pipeline::{
    DEBUG, LIGHTWEIGHT, RESEARCH, asked, decision, draft, kept, planning, points, rows, save,
    stored, work,
};
use crate::support::{Jev, Stand, configure};
use serde_json::json;
use tect_domain::PIPELINE_NOTE;

/// `sizing` keeps everything; `pipeline` chooses Research for every node it is asked about.
fn research(name: &str) -> (&'static str, f64) {
    match name {
        size if size.ends_with(".size") => ("bounded", 0.7),
        advice if advice.ends_with(".advice") => ("keep", 0.7),
        pipeline if pipeline.ends_with(".pipeline") => (RESEARCH, 0.8),
        _ => ("requested", 0.7),
    }
}

fn revisions(nodes: &[serde_json::Value]) -> Vec<i64> {
    nodes
        .iter()
        .map(|node| node["revision"].as_i64().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_later_draft_is_asked_about_its_changed_and_added_work_nodes_only() {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(research));

    // The first draft: two new work nodes, both asked about.
    let first = draft(vec![
        work(1, DEBUG, "first reason"),
        work(2, LIGHTWEIGHT, "second reason"),
    ]);
    let (_, head, first_request) = save(&mut client, &head, first).await;
    let nodes = stored(&head);
    assert_eq!(revisions(&nodes), [1, 1]);
    assert_eq!(
        asked(&stand.jev.asked(".pipeline")[0]),
        ["c1.pipeline", "c2.pipeline"]
    );

    // The second draft keeps the first node as it is, changes the second and adds a
    // third. The store keeps revision 1, writes revision 2 and writes a new node.
    let mut changed = kept(&nodes[1]);
    changed["outcome"] = json!("The cause is demonstrated on a fixed input");
    changed["change_rationale"] = json!("The reproduction needs a fixed input");
    let second = draft(vec![
        kept(&nodes[0]),
        changed,
        work(3, DEBUG, "third reason"),
    ]);
    let (saved, head, second_request) = save(&mut client, &head, second).await;
    let nodes = stored(&head);
    assert_eq!(revisions(&nodes), [1, 2, 1]);
    assert_eq!(
        nodes[1]["outcome"],
        "The cause is demonstrated on a fixed input"
    );

    // `sizing` asked about all three work nodes; `pipeline` about the two written ones.
    let (sizing, pipeline) = (stand.jev.asked(".size"), stand.jev.asked(".pipeline"));
    assert_eq!((sizing.len(), pipeline.len()), (2, 2));
    assert_eq!(
        asked(&sizing[1]),
        [
            "c1.advice",
            "c1.i1",
            "c1.size",
            "c2.advice",
            "c2.i1",
            "c2.size",
            "c3.advice",
            "c3.i1",
            "c3.size"
        ]
    );
    assert_eq!(asked(&pipeline[1]), ["c2.pipeline", "c3.pipeline"]);
    // Every node of the draft is still shown, the kept one too.
    assert_eq!(pipeline[1]["state"], sizing[1]["state"]);
    assert_eq!(
        pipeline[1]["state"]["candidates"].as_array().unwrap().len(),
        3
    );
    assert_eq!(
        saved["pipeline_advice"],
        json!({"status":"advised","note":PIPELINE_NOTE,"candidates":[
            {"candidate":2,"pipeline":RESEARCH,"pipeline_p":0.8,
             "draft_pipeline_p":0.02,"differs":true},
            {"candidate":3,"pipeline":RESEARCH,"pipeline_p":0.8,
             "draft_pipeline_p":0.02,"differs":true}
        ]}),
        "{saved}"
    );
    assert_eq!(
        saved["advice"]["candidates"].as_array().unwrap().len(),
        3,
        "{saved}"
    );
    for request in [first_request, second_request] {
        assert_eq!(points(&rows(&stand, request).await), ["sizing", "pipeline"]);
    }

    // The third draft keeps the three work nodes and adds a decision node: nothing
    // that is work was created or changed, so the point does not fire.
    let third = draft(vec![
        kept(&nodes[0]),
        kept(&nodes[1]),
        kept(&nodes[2]),
        decision("path", "Is Lightweight sufficient?"),
    ]);
    let calls = stand.jev.calls();
    let (saved, head, third_request) = save(&mut client, &head, third).await;
    let nodes = stored(&head);
    assert_eq!(revisions(&nodes), [1, 2, 1, 1]);
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    assert!(saved.get("pipeline_advice").is_none(), "{saved}");
    assert_eq!(points(&rows(&stand, third_request).await), ["sizing"]);
    assert_eq!(stand.jev.calls(), calls + 1);
    assert_eq!(stand.jev.asked(".pipeline").len(), 2);

    // The fourth draft changes that decision node and nothing else: still no work
    // node was written.
    let mut reworded = kept(&nodes[3]);
    reworded["question"] = json!("Is a Lightweight correction sufficient?");
    reworded["change_rationale"] = json!("Says what is corrected");
    let fourth = draft(vec![
        kept(&nodes[0]),
        kept(&nodes[1]),
        kept(&nodes[2]),
        reworded,
    ]);
    let (saved, head, fourth_request) = save(&mut client, &head, fourth).await;
    let nodes = stored(&head);
    assert_eq!(revisions(&nodes), [1, 2, 1, 2]);
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    assert!(saved.get("pipeline_advice").is_none(), "{saved}");
    assert_eq!(points(&rows(&stand, fourth_request).await), ["sizing"]);
    assert_eq!(stand.jev.asked(".pipeline").len(), 2);
    assert_eq!(stand.rows_of("pipeline").await, (2, 2));

    // The fifth draft changes nothing of the first work node but the pipeline the
    // agent chose for it: that is a changed node, and the only one asked about.
    let mut rechosen = kept(&nodes[0]);
    rechosen["pipeline"] = json!(LIGHTWEIGHT);
    rechosen["pipeline_reason"] = json!("The cause is known by now");
    rechosen["change_rationale"] = json!("A bounded correction is enough");
    let fifth = draft(vec![
        rechosen,
        kept(&nodes[1]),
        kept(&nodes[2]),
        kept(&nodes[3]),
    ]);
    let (saved, head, fifth_request) = save(&mut client, &head, fifth).await;
    assert_eq!(revisions(&stored(&head)), [2, 2, 1, 2]);
    assert_eq!(stored(&head)[0]["pipeline"], LIGHTWEIGHT);
    let pipeline = stand.jev.asked(".pipeline");
    assert_eq!(pipeline.len(), 3);
    assert_eq!(asked(&pipeline[2]), ["c1.pipeline"]);
    assert_eq!(
        saved["pipeline_advice"]["candidates"],
        json!([{"candidate":1,"pipeline":RESEARCH,"pipeline_p":0.8,
            "draft_pipeline_p":0.02,"differs":true}]),
        "{saved}"
    );
    assert_eq!(
        points(&rows(&stand, fifth_request).await),
        ["sizing", "pipeline"]
    );
    assert_eq!(stand.rows_of("pipeline").await, (3, 3));
    client.finish().await;
}
