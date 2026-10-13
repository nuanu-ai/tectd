//! One point without advice does not touch the other, and the save is stored either
//! way; and what a disabled Jev, a Program without a mode and the daily limit give.
use crate::pipeline::{DEBUG, SAVE, draft, enable, planning, points, rows, save, stored, work};
use crate::support::{Jev, Stand, command, configure};
use serde_json::{Value, json};
use tect_domain::PIPELINE_NOTE;

/// `sizing` keeps everything; `pipeline` agrees with a draft that chose Debug.
fn settled(name: &str) -> (&'static str, f64) {
    match name {
        size if size.ends_with(".size") => ("bounded", 0.7),
        advice if advice.ends_with(".advice") => ("keep", 0.7),
        pipeline if pipeline.ends_with(".pipeline") => (DEBUG, 0.8),
        _ => ("requested", 0.7),
    }
}

fn two_nodes() -> Value {
    draft(vec![work(1, DEBUG, "first"), work(2, DEBUG, "second")])
}

const NOT_AN_ANSWER: &str = "Jev advice is not available: the answer has no `answers` object";

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn one_point_failing_leaves_the_other_advised_and_the_draft_stored() {
    let stand = Stand::start("2000").await;

    // Jev answers `sizing` and fails `pipeline`.
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::AnswersBut(settled, ".pipeline"));
    let (saved, next, request_id) = save(&mut client, &head, two_nodes()).await;
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    assert_eq!(saved["advice"]["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(
        saved["pipeline_advice"],
        json!({"status":"unavailable","reason":"invalid_response","message":NOT_AN_ANSWER}),
        "{saved}"
    );
    assert_eq!(stored(&next).len(), 2);
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(written[0]["outcome"], "advised");
    assert_eq!(written[1]["advice"], saved["pipeline_advice"]);
    assert_eq!(
        (
            &written[1]["called"],
            &written[1]["outcome"],
            &written[1]["reason"],
            &written[1]["send_certainty"]
        ),
        (
            &json!(true),
            &json!("unavailable"),
            &json!("invalid_response"),
            &json!("sent")
        )
    );

    // The other way round in another workspace: `sizing` fails, `pipeline` is advised.
    let (mut other, repo) = stand.workspace().await;
    let (head, _) = planning(&mut other, &repo).await;
    configure(&mut other, true, 0).await;
    stand.jev.answer(Jev::AnswersBut(settled, ".size"));
    let (saved, next, request_id) = save(&mut other, &head, two_nodes()).await;
    assert_eq!(
        saved["advice"],
        json!({"status":"unavailable","reason":"invalid_response","message":NOT_AN_ANSWER}),
        "{saved}"
    );
    assert_eq!(
        saved["pipeline_advice"],
        json!({"status":"advised","note":PIPELINE_NOTE,"candidates":[
            {"candidate":1,"pipeline":DEBUG,"pipeline_p":0.8,"draft_pipeline_p":0.8,
             "differs":false},
            {"candidate":2,"pipeline":DEBUG,"pipeline_p":0.8,"draft_pipeline_p":0.8,
             "differs":false}
        ]}),
        "{saved}"
    );
    assert_eq!(stored(&next).len(), 2);
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(
        (&written[0]["outcome"], &written[1]["outcome"]),
        (&json!("unavailable"), &json!("advised"))
    );
    assert_eq!(stand.jev.calls(), 4);
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn the_daily_limit_reached_between_the_two_points_stops_the_second() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    // One call a day: `sizing` takes it.
    enable(&mut client, 1, 0).await;
    let (saved, next, request_id) = save(&mut client, &head, two_nodes()).await;
    assert_eq!(saved["advice"]["status"], "advised", "{saved}");
    assert_eq!(
        saved["pipeline_advice"],
        json!({"status":"unavailable","reason":"daily_limit_reached",
            "message":"Jev advice is not available: the workspace daily call limit of 1 \
                is reached"}),
        "{saved}"
    );
    assert_eq!(stored(&next).len(), 2);
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
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_program_without_a_mode_gets_mode_not_set_from_the_point() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let (head, program_id) = planning(&mut client, &repo).await;
    stand.set_mode(&program_id, None).await;
    configure(&mut client, true, 0).await;
    let (saved, next, request_id) = save(&mut client, &head, two_nodes()).await;
    let without_mode = json!({"status":"unavailable","reason":"mode_not_set",
        "message":"Jev advice is not available: the Program has no mode; \
            set it with program.save"});
    assert_eq!(saved["pipeline_advice"], without_mode, "{saved}");
    assert_eq!(saved["advice"], without_mode, "{saved}");
    assert_eq!(stored(&next).len(), 2);
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    assert_eq!(written[1]["advice"], saved["pipeline_advice"]);
    assert_eq!(
        (
            &written[1]["called"],
            &written[1]["send_certainty"],
            &written[1]["reason"],
            &written[1]["request"]
        ),
        (
            &json!(false),
            &json!("not_sent"),
            &json!("mode_not_set"),
            &Value::Null
        )
    );
    assert_eq!(written[1]["program_id"], program_id);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn a_disabled_jev_adds_no_field_and_writes_no_row() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    let params = head.save(two_nodes());

    // No setting row, then a setting that says disabled: the same reply, byte for
    // byte, with neither field, and nothing written or asked.
    let (plain, plain_bytes) = command(&mut client, SAVE, params.clone()).await;
    assert!(plain.get("advice").is_none() && plain.get("pipeline_advice").is_none());
    configure(&mut client, false, 0).await;
    let (disabled, disabled_bytes) = command(&mut client, SAVE, params.clone()).await;
    assert_eq!(disabled, plain);
    assert_eq!(disabled.to_string(), plain.to_string());
    assert_eq!(disabled_bytes, plain_bytes);
    assert_eq!(stand.count("advice_journal").await, 0);
    assert_eq!(stand.jev.calls(), 0);

    // Enabled: the same reply with exactly the two fields more.
    configure(&mut client, true, 1).await;
    let (enabled, _) = command(&mut client, SAVE, params.clone()).await;
    let mut without = enabled.clone();
    let fields = without.as_object_mut().unwrap();
    assert_eq!(fields.remove("advice").unwrap()["status"], "advised");
    assert_eq!(
        fields.remove("pipeline_advice").unwrap()["status"],
        "advised"
    );
    assert_eq!(without, plain);
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        (2, 2)
    );

    // Disabled again: neither stored field is shown and nothing more is written.
    configure(&mut client, false, 2).await;
    let (again, again_bytes) = command(&mut client, SAVE, params).await;
    assert_eq!(again.to_string(), plain.to_string());
    assert_eq!(again_bytes, plain_bytes);
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        (2, 2)
    );
    client.finish().await;
}
