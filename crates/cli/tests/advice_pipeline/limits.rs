//! Drafts at the limits of the reply and of the request: what each point does with
//! them is decided for that point alone.
use crate::pipeline::{DEBUG, SAVE, draft, planning, points, rows, save, stored, work};
use crate::support::{Jev, Stand, configure};
use serde_json::{Value, json};
use tect_domain::PIPELINE_NOTE;

fn settled(name: &str) -> (&'static str, f64) {
    match name {
        size if size.ends_with(".size") => ("bounded", 0.7),
        advice if advice.ends_with(".advice") => ("keep", 0.7),
        pipeline if pipeline.ends_with(".pipeline") => (DEBUG, 0.8),
        _ => ("requested", 0.7),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn sizing_advice_too_large_for_the_reply_does_not_stop_the_pipeline_point() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;

    // One work node with two thousand included items: the largest `sizing` field
    // lists every one of them and cannot fit; the `pipeline` field has one entry.
    let mut wide = work(1, DEBUG, "first");
    wide["includes"] = json!(
        (1..=2000)
            .map(|item| format!("item {item}"))
            .collect::<Vec<_>>()
    );
    let (saved, next, request_id) = save(&mut client, &head, draft(vec![wide])).await;
    assert_eq!(saved["advice"]["reason"], "advice_too_large", "{saved}");
    assert_eq!(
        saved["pipeline_advice"],
        json!({"status":"advised","note":PIPELINE_NOTE,"candidates":[
            {"candidate":1,"pipeline":DEBUG,"pipeline_p":0.8,"draft_pipeline_p":0.8,
             "differs":false}
        ]}),
        "{saved}"
    );
    assert_eq!(stored(&next)[0]["includes"].as_array().unwrap().len(), 2000);
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(
        (&written[0]["called"], &written[1]["called"]),
        (&json!(false), &json!(true))
    );
    // Jev was asked once, by `pipeline`.
    assert_eq!(stand.jev.calls(), 1);
    assert_eq!(stand.jev.asked(".pipeline").len(), 1);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_draft_over_the_request_ceiling_reaches_jev_from_neither_point() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;

    // The state both points send holds this outcome, and it alone is over 256 KiB.
    let mut long = work(1, DEBUG, "first");
    long["outcome"] = json!("A long outcome. ".repeat(20_000));
    let (saved, next, request_id) = save(&mut client, &head, draft(vec![long])).await;
    for field in ["advice", "pipeline_advice"] {
        assert_eq!(saved[field]["status"], "unavailable", "{field}");
        assert_eq!(saved[field]["reason"], "request_too_large", "{field}");
        let message = saved[field]["message"].as_str().unwrap();
        assert!(
            message.starts_with("Jev advice is not available: the request of ")
                && message.ends_with(" bytes exceeds the limit of 262144 bytes"),
            "{field}: {message}"
        );
    }
    assert_eq!(stored(&next).len(), 1);
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    for row in &written {
        assert_eq!(
            (&row["called"], &row["send_certainty"], &row["request"]),
            (&json!(false), &json!("not_sent"), &Value::Null),
            "{}",
            row["point"]
        );
    }
    assert_eq!(written[1]["advice"], saved["pipeline_advice"]);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}

/// A first draft of this many new work nodes without included items.
fn many(count: usize) -> Value {
    draft(
        (1..=count)
            .map(|number| {
                let mut node = work(number, DEBUG, "reason");
                node["includes"] = json!([]);
                node
            })
            .collect(),
    )
}

fn too_large(asked: usize) -> Value {
    json!({"status":"unavailable","reason":"advice_too_large",
        "message":format!("Jev advice is not available: advice for the pipelines of this \
            draft (work nodes asked about: {asked}) would not fit the command reply")})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn pipeline_advice_that_would_not_fit_beside_the_sizing_field_is_not_asked_for() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));

    // Forty new work nodes: `sizing` has room for its field and is advised; with that
    // field in the reply the largest `pipeline_advice` for forty nodes does not fit.
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    let (saved, next, request_id) = save(&mut client, &head, many(40)).await;
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    assert_eq!(saved["advice"]["candidates"].as_array().unwrap().len(), 40);
    assert_eq!(saved["pipeline_advice"], too_large(40), "{saved}");
    assert_eq!(stored(&next).len(), 40);
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(written[1]["advice"], saved["pipeline_advice"]);
    assert_eq!(
        (
            &written[0]["called"],
            &written[1]["called"],
            &written[1]["send_certainty"],
            &written[1]["request"]
        ),
        (
            &json!(true),
            &json!(false),
            &json!("not_sent"),
            &Value::Null
        )
    );
    assert_eq!(stand.jev.calls(), 1);
    assert!(stand.jev.asked(".pipeline").is_empty());

    // Forty-eight, in another workspace: more than either field can cover, each
    // measured for itself. Jev is not asked at all.
    let (mut other, repo) = stand.workspace().await;
    let (head, _) = planning(&mut other, &repo).await;
    configure(&mut other, true, 0).await;
    let (saved, next, request_id) = save(&mut other, &head, many(48)).await;
    assert_eq!(saved["advice"]["reason"], "advice_too_large", "{saved}");
    assert_eq!(saved["pipeline_advice"], too_large(48), "{saved}");
    assert_eq!(stored(&next).len(), 48);
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(
        (&written[0]["called"], &written[1]["called"]),
        (&json!(false), &json!(false))
    );
    assert_eq!(stand.jev.calls(), 1);
    client.finish().await;
    other.finish().await;
}

/// Every `sizing` answer at its longest token and widest probability and every
/// included item extra: the field `sizing` gives is the largest it can give.
fn longest(name: &str) -> (&'static str, f64) {
    match name {
        size if size.ends_with(".size") => ("oversized", 0.99),
        advice if advice.ends_with(".advice") => ("separate_scope", 0.99),
        pipeline if pipeline.ends_with(".pipeline") => (DEBUG, 0.99),
        _ => ("extra", 0.99),
    }
}

/// KNOWN DEFECT, pinned as it behaves today and awaiting a decision; this test does
/// not say the behaviour is wanted. `sizing` may fill the reply to within less than
/// the shortest field the second point can give (its `advice_too_large`, 225 bytes).
/// The save is stored, Jev is called for `sizing`, both rows are journalled, and the
/// command answers with a transport-capacity error instead of the reply; a replay
/// answers with the same error. Reached here with thirty-seven new work nodes of
/// five included items and every answer of Jev at its longest; one node fewer or
/// more and the reply leaves as it should.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn known_defect_a_full_sizing_field_leaves_no_room_for_any_pipeline_field() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(longest));
    let full = |count: usize| {
        draft(
            (1..=count)
                .map(|number| {
                    let mut node = work(number, DEBUG, "reason");
                    node["includes"] = json!(["a", "b", "c", "d", "e"]);
                    node
                })
                .collect(),
        )
    };

    // One node below: `sizing` is advised and the second point's short field fits.
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    let (saved, _, _) = save(&mut client, &head, full(36)).await;
    assert_eq!(saved["advice"]["candidates"].as_array().unwrap().len(), 36);
    assert_eq!(saved["pipeline_advice"], too_large(36), "{saved}");

    // At thirty-seven the command answers with an error although the draft is stored.
    let (mut other, repo) = stand.workspace().await;
    let (head, _) = planning(&mut other, &repo).await;
    configure(&mut other, true, 0).await;
    let params = head.save(full(37));
    let request_id = uuid::Uuid::parse_str(params["request_id"].as_str().unwrap()).unwrap();
    for _ in 0..2 {
        let call = json!({"name":"command","arguments":{"route":SAVE,"params":params.clone()}});
        let response = other.exchange("tools/call", call).await;
        assert_eq!(response["result"]["isError"], true, "{response}");
        let text = response["result"]["content"][1]["text"].as_str().unwrap();
        let error: Value = serde_json::from_str(text).unwrap();
        assert_eq!(error["error"]["code"], "request_too_large", "{error}");
        assert!(error.get("advice").is_none() && error.get("pipeline_advice").is_none());
    }
    let drafts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM slice_candidate_drafts WHERE tenant_id=$1 AND candidate_set_id=$2",
    )
    .bind(stand.tenant)
    .bind(uuid::Uuid::parse_str(head.context["candidate_set"]["id"].as_str().unwrap()).unwrap())
    .fetch_one(&stand.pool)
    .await
    .unwrap();
    assert_eq!(drafts, 1, "the draft is stored");
    // The journal holds two fields the agent never received, and one paid call.
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(
        (&written[0]["called"], &written[0]["outcome"]),
        (&json!(true), &json!("advised"))
    );
    assert_eq!(written[1]["advice"], too_large(37));
    assert_eq!(written[1]["called"], false);
    assert_eq!(stand.jev.calls(), 2);
    client.finish().await;
    other.finish().await;
}
