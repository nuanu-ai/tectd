//! An enabled Jev that gives no advice: the draft is stored all the same and the
//! reply says why there is none.
use crate::drafts::{Level, Shape, target};
use crate::support::{BUDGET, Jev, Stand, command, configure};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

/// What the journal says about the newest point: called, send certainty, reason.
async fn journalled(stand: &Stand, advice: &Value) -> (Value, Value, Value) {
    let row = stand.newest_row().await;
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
    // The replay returns the recorded outcome and does not ask again.
    let (replayed, _) = command(&mut client, level.save_route(), params).await;
    assert_eq!(replayed, saved);
    assert_eq!(stand.jev.calls(), 1);
    assert_eq!(stand.count("advice_journal").await, 1);

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
    assert_eq!(stand.jev.calls(), 2);
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
    assert_eq!(stand.newest_row().await["request"], Value::Null);
    let (replayed, _) = command(&mut client, level.save_route(), params).await;
    assert_eq!(replayed, saved);
    assert_eq!(stand.count("advice_journal").await, 1);

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

    // Neither draft reached Jev, and neither spent the daily limit.
    assert_eq!(stand.jev.calls(), 0);
    let called: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advice_journal WHERE tenant_id=$1 AND called")
            .bind(stand.tenant)
            .fetch_one(&stand.pool)
            .await
            .unwrap();
    assert_eq!(called, 0);
    client.finish().await;
    other.finish().await;
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
