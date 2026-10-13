//! An enabled Jev that gives no advice: the draft is stored all the same and the
//! reply says why there is none.
use crate::drafts::{Level, Shape, target};
use crate::support::{BUDGET, Jev, Stand, audit, command, configure};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

/// What the journal says about the newest `sizing` row: called, send certainty, reason.
async fn journalled(stand: &Stand, advice: &Value) -> (Value, Value, Value) {
    let row = stand.newest_row_of("sizing").await;
    assert_eq!(row["point"], "sizing");
    assert_eq!(row["outcome"], "unavailable");
    assert_eq!(&row["advice"], advice);
    assert!(advice.get("note").is_none() && advice.get("candidates").is_none());
    (
        row["called"].clone(),
        row["send_certainty"].clone(),
        row["reason"].clone(),
    )
}

async fn a_failing_jev_leaves_the_draft_stored(level: Level) {
    let stand = Stand::start("800").await;

    // Jev answers after the daemon has stopped waiting.
    let (mut client, repo) = stand.workspace().await;
    let target_late = target(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Late(Duration::from_millis(2500)));
    let before = target_late.set()["revision"].as_i64().unwrap();
    let params = target_late.save(&Shape::of(1, 1), Uuid::new_v4());
    let (saved, envelope) = command(&mut client, level.save_route(), params.clone()).await;
    assert_eq!(
        saved["advice"],
        json!({"status":"unavailable","reason":"timeout",
            "message":"Jev advice is not available: Jev did not answer within 800 ms"})
    );
    assert!(envelope <= BUDGET);
    assert_eq!(saved["candidate_set"]["revision"], before + 1);
    assert_eq!(saved["candidate_set"]["status"], "review_required");
    let stored = target_late.stored(&mut client, &saved).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0]["outcome"], "The cause is demonstrated");
    assert_eq!(
        journalled(&stand, &saved["advice"]).await,
        (json!(true), json!("sent_unknown"), json!("timeout"))
    );
    // The replay returns the recorded outcome and does not ask again, for any point.
    let before = (stand.jev.calls(), stand.count("advice_journal").await);
    let (replayed, _) = command(&mut client, level.save_route(), params).await;
    assert_eq!(replayed, saved);
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        before
    );
    assert_eq!(stand.jev.asked(".size").len(), 1);
    assert_eq!(stand.rows_of("sizing").await, (1, 1));

    // Jev answers in time with something that is not an answer.
    let (mut other, repo) = stand.workspace().await;
    let target_bad = target(&mut other, &repo, level).await;
    configure(&mut other, true, 0).await;
    stand.jev.answer(Jev::Garbage);
    let params = target_bad.save(&Shape::of(1, 1), Uuid::new_v4());
    let (saved, _) = command(&mut other, level.save_route(), params).await;
    assert_eq!(
        saved["advice"],
        json!({"status":"unavailable","reason":"invalid_response",
            "message":"Jev advice is not available: the answer has no `answers` object"})
    );
    assert_eq!(target_bad.stored(&mut other, &saved).await.len(), 1);
    assert_eq!(
        journalled(&stand, &saved["advice"]).await,
        (json!(true), json!("sent"), json!("invalid_response"))
    );
    assert_eq!(stand.jev.asked(".size").len(), 2);
    client.finish().await;
    other.finish().await;
}

async fn a_draft_jev_cannot_be_asked_about_is_stored_without_a_call(level: Level) {
    let stand = Stand::start("2000").await;

    // The largest advice this draft could get would not fit the save reply.
    let (mut client, repo) = stand.workspace().await;
    let target_wide = target(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    let params = target_wide.save(&Shape::of(1, 2000), Uuid::new_v4());
    let (saved, envelope) = command(&mut client, level.save_route(), params.clone()).await;
    assert_eq!(
        saved["advice"],
        json!({"status":"unavailable","reason":"advice_too_large",
            "message":"Jev advice is not available: advice for this draft (1 candidate, \
                2000 included items) would not fit the command reply"})
    );
    assert!(envelope <= BUDGET, "{envelope}");
    let stored = target_wide.stored(&mut client, &saved).await;
    assert_eq!(stored[0]["includes"].as_array().unwrap().len(), 2000);
    assert_eq!(
        journalled(&stand, &saved["advice"]).await,
        (json!(false), json!("not_sent"), json!("advice_too_large"))
    );
    assert_eq!(stand.newest_row_of("sizing").await["request"], Value::Null);
    let before = (stand.jev.calls(), stand.count("advice_journal").await);
    let (replayed, _) = command(&mut client, level.save_route(), params).await;
    assert_eq!(replayed, saved);
    assert_eq!(
        (stand.jev.calls(), stand.count("advice_journal").await),
        before
    );
    assert_eq!(stand.rows_of("sizing").await, (1, 0));

    // The draft fits the reply, and its text is over the request ceiling of Jev.
    let (mut other, repo) = stand.workspace().await;
    let target_long = target(&mut other, &repo, level).await;
    configure(&mut other, true, 0).await;
    let mut shape = Shape::of(1, 1);
    shape.outcome = "A long outcome. ".repeat(20_000);
    let params = target_long.save(&shape, Uuid::new_v4());
    let (saved, envelope) = command(&mut other, level.save_route(), params).await;
    assert_eq!(saved["advice"]["status"], "unavailable");
    assert_eq!(saved["advice"]["reason"], "request_too_large");
    let message = saved["advice"]["message"].as_str().unwrap();
    assert!(
        message.starts_with("Jev advice is not available: the request of ")
            && message.ends_with(" bytes exceeds the limit of 262144 bytes"),
        "{message}"
    );
    assert!(envelope <= BUDGET);
    assert_eq!(saved["candidate_set"]["status"], "review_required");
    assert_eq!(
        journalled(&stand, &saved["advice"]).await,
        (json!(false), json!("not_sent"), json!("request_too_large"))
    );

    // Neither draft reached Jev for `sizing`, and neither spent the daily limit on it.
    // What the `pipeline` point does with these two drafts is its own suite's matter.
    assert!(stand.jev.asked(".size").is_empty());
    assert_eq!(stand.rows_of("sizing").await, (2, 0));
    client.finish().await;
    other.finish().await;
}

/// A Program opened before the product level existed has none, and every request
/// carries the level: Jev is not asked until `program.save` states one.
async fn a_draft_of_a_program_without_a_mode_is_stored_without_a_call(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(|_| ("ABSTAIN", 0.9)));
    let (mut client, repo) = stand.workspace().await;
    let target_legacy = target(&mut client, &repo, level).await;
    stand.set_mode(&target_legacy.program_id, None).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = target_legacy.save(&Shape::of(2, 2), request_id);
    let (saved, envelope) = command(&mut client, level.save_route(), params.clone()).await;
    assert_eq!(
        saved["advice"],
        json!({"status":"unavailable","reason":"mode_not_set",
            "message":"Jev advice is not available: the Program has no mode; \
                set it with program.save"})
    );
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(saved["candidate_set"]["status"], "review_required");
    assert_eq!(target_legacy.stored(&mut client, &saved).await.len(), 2);

    // One journal row for the point that fired, with the work it was for and no call.
    assert_eq!(
        journalled(&stand, &saved["advice"]).await,
        (json!(false), json!("not_sent"), json!("mode_not_set"))
    );
    let row = stand.newest_row_of("sizing").await;
    assert_eq!(row["request"], Value::Null);
    assert_eq!(row["route"], level.save_route());
    assert_eq!(row["request_id"], json!(request_id));
    assert_eq!(row["program_id"], target_legacy.program_id);
    assert_eq!(row["candidate_set_id"], target_legacy.set()["id"]);
    let scope_id = match level {
        Level::Scopes => Value::Null,
        Level::Slices => target_legacy.context["scope"]["id"].clone(),
    };
    assert_eq!(row["scope_id"], scope_id);
    let counters = audit(&mut client).await["counters"].clone();
    assert_eq!(
        (&counters["called"], &counters["not_called"]),
        (&json!(0), &json!(1))
    );
    assert_eq!(counters["reasons"], json!({"mode_not_set":1}));

    // A replay returns the same reply, also once the Program has a mode: no call, no row.
    let (replayed, _) = command(&mut client, level.save_route(), params.clone()).await;
    assert_eq!(replayed, saved);
    stand.set_mode(&target_legacy.program_id, Some("mvp")).await;
    let (replayed, _) = command(&mut client, level.save_route(), params).await;
    assert_eq!(replayed, saved);
    let rows = stand.count("advice_journal").await;
    assert_eq!(stand.rows_of("sizing").await, (1, 0));

    // Disabled is decided first: without a mode and without Jev the save answers as
    // `main` does, with no field and no row.
    let (mut other, repo) = stand.workspace().await;
    let target_off = target(&mut other, &repo, level).await;
    stand.set_mode(&target_off.program_id, None).await;
    let params = target_off.save(&Shape::of(2, 2), Uuid::new_v4());
    let (plain, _) = command(&mut other, level.save_route(), params).await;
    assert!(plain.get("advice").is_none(), "{plain}");
    assert!(plain.get("pipeline_advice").is_none(), "{plain}");
    assert_eq!(stand.count("advice_journal").await, rows);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_draft_of_a_program_without_a_mode_is_not_sent_to_jev() {
    a_draft_of_a_program_without_a_mode_is_stored_without_a_call(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_draft_of_a_program_without_a_mode_is_not_sent_to_jev() {
    a_draft_of_a_program_without_a_mode_is_stored_without_a_call(Level::Slices).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_draft_is_stored_when_jev_times_out_or_answers_badly() {
    a_failing_jev_leaves_the_draft_stored(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_draft_is_stored_when_jev_times_out_or_answers_badly() {
    a_failing_jev_leaves_the_draft_stored(Level::Slices).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_draft_too_large_for_the_reply_or_the_request_is_not_sent() {
    a_draft_jev_cannot_be_asked_about_is_stored_without_a_call(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_draft_too_large_for_the_reply_or_the_request_is_not_sent() {
    a_draft_jev_cannot_be_asked_about_is_stored_without_a_call(Level::Slices).await;
}
