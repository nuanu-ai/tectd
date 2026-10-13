//! Later drafts: Jev is asked only about the candidates a save created or changed,
//! and a save that created and changed none does not fire the point.
use crate::drafts::{Level, target};
use crate::support::{Jev, Stand, command, configure};
use crate::walk::{
    ScopeSet, SliceSet, retained, scope_candidate, scope_draft, scope_resave, slice_draft,
    work_node,
};
use serde_json::{Value, json};

fn keeping(name: &str) -> (&'static str, f64) {
    match name.rsplit('.').next() {
        Some("size") => ("bounded", 0.7),
        Some("advice") => ("keep", 0.7),
        // The second point of a Slice draft save; what it answers is its own suite's.
        Some("pipeline") => ("ABSTAIN", 0.7),
        _ => ("requested", 0.7),
    }
}

/// The names of the questions of one request to Jev, in the order it holds them.
fn asked(request: &Value) -> Vec<&String> {
    request["questions"].as_object().unwrap().keys().collect()
}

/// The draft positions the `advice` field of a reply has an entry for.
fn advised(reply: &Value) -> Vec<i64> {
    reply["advice"]["candidates"]
        .as_array()
        .unwrap_or_else(|| panic!("{reply}"))
        .iter()
        .map(|entry| entry["candidate"].as_i64().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_later_scope_draft_is_asked_about_its_created_and_changed_candidates_only() {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = ScopeSet::begun(&mut client, &repo).await;
    let refs = head.input_refs(&mut client).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(keeping));
    let route = Level::Scopes.save_route();

    // The first draft: two new candidates, both asked about.
    let first = scope_draft(&refs[0], vec![scope_candidate(1), scope_candidate(2)]);
    let (saved, _) = command(&mut client, route, head.save(first)).await;
    assert_eq!(advised(&saved), [1, 2], "{saved}");
    let head = ScopeSet::after(&mut client, saved).await;

    // The second draft keeps the first candidate as it is, changes the second and
    // adds a third. The store keeps revision 1, writes revision 2 and a new candidate.
    let mut second = scope_resave(&head.details(&mut client).await, &refs[0]);
    second["candidates"][1]["outcome"] = json!("The cause is demonstrated on a fixed input");
    second["candidates"][1]["change_rationale"] = json!("The reproduction needs a fixed input");
    second["goals"].as_array_mut().unwrap().push(json!({
        "identity":{"local":"goal-3"},"text":"Goal 3","source_ref_id":refs[0],
        "resolution":{"kind":"candidate","reference":{"local":"scope-3"}}}));
    second["candidates"]
        .as_array_mut()
        .unwrap()
        .push(scope_candidate(3));
    let (saved, _) = command(&mut client, route, head.save(second)).await;
    let head = ScopeSet::after(&mut client, saved.clone()).await;
    let (_, stored) = head.candidates(&mut client).await;
    let revisions = stored
        .iter()
        .map(|candidate| candidate["revision"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(revisions, [1, 2, 1]);

    // Jev read all three candidates and was asked about the two the save wrote; the
    // field has one entry for each, under its position in the whole draft.
    let requests = stand.jev.asked(".size");
    assert_eq!(requests.len(), 2);
    assert_eq!(
        asked(&requests[1]),
        [
            "c2.advice",
            "c2.i1",
            "c2.size",
            "c3.advice",
            "c3.i1",
            "c3.size"
        ]
    );
    assert_eq!(
        requests[1]["state"]["candidates"].as_array().unwrap().len(),
        3
    );
    assert_eq!(requests[1]["state"]["candidates"][0]["key"], "c1");
    assert_eq!(advised(&saved), [2, 3], "{saved}");
    assert_eq!(stand.rows_of("sizing").await, (2, 2));

    // The third draft states the three candidates again as they are: the save created
    // and changed none, so there is no field, no journal row and no call.
    let third = scope_resave(&head.details(&mut client).await, &refs[0]);
    let before = (stand.jev.calls(), stand.count("advice_journal").await);
    let (saved, _) = command(&mut client, route, head.save(third)).await;
    assert_eq!(saved["candidate_set"]["status"], "review_required");
    assert!(saved.get("advice").is_none(), "{saved}");
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        before
    );
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_later_slice_draft_is_asked_about_its_created_and_changed_work_nodes_only() {
    let stand = Stand::start("2000").await;
    let (mut client, repo) = stand.workspace().await;
    let head = SliceSet {
        context: target(&mut client, &repo, Level::Slices).await.context,
    };
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Answers(keeping));
    let route = Level::Slices.save_route();

    // The first draft: two new work nodes, both asked about.
    let first = slice_draft(vec![work_node(1), work_node(2)]);
    let (saved, _) = command(&mut client, route, head.save(first)).await;
    assert_eq!(advised(&saved), [1, 2], "{saved}");
    let head = SliceSet::after(&mut client, saved).await;
    let nodes = head.context["draft"]["nodes"].as_array().unwrap().clone();

    // The second draft keeps the first node, changes the second and adds a third.
    let mut changed = retained(&nodes[1]);
    changed["outcome"] = json!("The cause is demonstrated on a fixed input");
    changed["change_rationale"] = json!("The reproduction needs a fixed input");
    let second = slice_draft(vec![retained(&nodes[0]), changed, work_node(3)]);
    let (saved, _) = command(&mut client, route, head.save(second)).await;
    let requests = stand.jev.asked(".size");
    assert_eq!(requests.len(), 2);
    assert_eq!(
        asked(&requests[1]),
        [
            "c2.advice",
            "c2.i1",
            "c2.size",
            "c3.advice",
            "c3.i1",
            "c3.size"
        ]
    );
    assert_eq!(
        requests[1]["state"]["candidates"].as_array().unwrap().len(),
        3
    );
    assert_eq!(advised(&saved), [2, 3], "{saved}");
    let head = SliceSet::after(&mut client, saved).await;
    let nodes = head.context["draft"]["nodes"].as_array().unwrap().clone();
    assert_eq!(stand.rows_of("sizing").await, (2, 2));

    // The third draft states the three nodes again as they are: no work node was
    // created or changed, so neither point of the save fires.
    let third = slice_draft(nodes.iter().map(retained).collect());
    let before = (stand.jev.calls(), stand.count("advice_journal").await);
    let (saved, _) = command(&mut client, route, head.save(third)).await;
    assert_eq!(saved["candidate_set"]["status"], "review_required");
    assert!(saved.get("advice").is_none(), "{saved}");
    assert!(saved.get("pipeline_advice").is_none(), "{saved}");
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        before
    );
    client.finish().await;
}
