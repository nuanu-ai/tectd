//! An enabled Jev that gives no advice: the input is recorded all the same and the
//! reply says why there is none. The `model` point of the same command meets the same
//! Jev and writes a row of its own; this suite reads the rows of `intake`.
use crate::drafts::Level;
use crate::inputs::{FIRST, Planned, audit, route};
use crate::support::{BUDGET, Jev, Stand, command, configure};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

/// What the journal says about the newest `intake` row: called, send certainty, reason.
async fn journalled(stand: &Stand, advice: &Value) -> (Value, Value, Value) {
    let row = stand.newest_row_of("intake").await;
    assert_eq!(row["point"], "intake");
    assert_eq!(row["outcome"], "unavailable");
    assert_eq!(&row["advice"], advice);
    assert!(advice.get("note").is_none() && advice.get("work").is_none());
    (
        row["called"].clone(),
        row["send_certainty"].clone(),
        row["reason"].clone(),
    )
}

async fn a_failing_jev_leaves_the_input_recorded(level: Level) {
    let stand = Stand::start("800").await;

    // Jev answers after the daemon has stopped waiting.
    let (mut client, repo) = stand.workspace().await;
    let late = Planned::bare(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Late(Duration::from_millis(2500)));
    let before = late.set()["revision"].as_i64().unwrap();
    let params = late.input(FIRST, Uuid::new_v4());
    let (recorded, envelope) = command(&mut client, route(level), params.clone()).await;
    assert_eq!(
        recorded["advice"],
        json!({"status":"unavailable","reason":"timeout",
            "message":"Jev advice is not available: Jev did not answer within 800 ms"})
    );
    assert!(envelope <= BUDGET);
    assert_eq!(recorded["candidate_set"]["revision"], before + 1);
    assert_eq!(recorded["candidate_set"]["status"], "review_required");
    assert_eq!(late.recorded(&stand).await.last().unwrap(), FIRST);
    assert_eq!(
        journalled(&stand, &recorded["advice"]).await,
        (json!(true), json!("sent_unknown"), json!("timeout"))
    );
    // The replay returns the recorded outcome and does not ask again. Each of the
    // two points of the command had asked once.
    let (replayed, _) = command(&mut client, route(level), params).await;
    assert_eq!(replayed, recorded);
    assert_eq!((stand.jev.calls(), stand.jev.asked("fit").len()), (2, 1));
    assert_eq!(stand.rows_of("intake").await, (1, 1));
    assert_eq!(stand.count("advice_journal").await, 2);

    // Jev answers in time with something that is not an answer.
    let (mut other, repo) = stand.workspace().await;
    let bad = Planned::bare(&mut other, &repo, level).await;
    configure(&mut other, true, 0).await;
    stand.jev.answer(Jev::Garbage);
    let params = bad.input(FIRST, Uuid::new_v4());
    let (recorded, _) = command(&mut other, route(level), params).await;
    assert_eq!(
        recorded["advice"],
        json!({"status":"unavailable","reason":"invalid_response",
            "message":"Jev advice is not available: the answer has no `answers` object"})
    );
    assert_eq!(bad.recorded(&stand).await.last().unwrap(), FIRST);
    assert_eq!(
        journalled(&stand, &recorded["advice"]).await,
        (json!(true), json!("sent"), json!("invalid_response"))
    );
    assert_eq!((stand.jev.calls(), stand.jev.asked("fit").len()), (4, 2));
    client.finish().await;
    other.finish().await;
}

/// The workspace enabled Jev and the daemon was started without a provider.
async fn a_daemon_without_a_provider_records_the_input_and_says_so(level: Level) {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::bare(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = planned.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, route(level), params.clone()).await;
    assert_eq!(
        recorded["advice"],
        json!({"status":"unavailable","reason":"provider_unconfigured",
            "message":"Jev advice is not available: the daemon has no Jev provider configured"})
    );
    assert!(envelope <= BUDGET);
    assert_eq!(recorded["candidate_set"]["status"], "review_required");
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    assert_eq!(
        journalled(&stand, &recorded["advice"]).await,
        (
            json!(false),
            json!("not_sent"),
            json!("provider_unconfigured")
        )
    );
    let row = stand.newest_row_of("intake").await;
    assert_eq!(row["request"], Value::Null);
    assert_eq!(row["route"], route(level));
    assert_eq!(row["request_id"], json!(request_id));
    let (replayed, _) = command(&mut client, route(level), params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.rows_of("intake").await, (1, 0));
    assert_eq!(stand.count("advice_journal").await, 2);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}

/// A Program opened before the product level existed has none, and every request
/// carries the level: Jev is not asked until `program.save` states one.
async fn an_input_of_a_program_without_a_mode_is_recorded_without_a_call(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(|_| ("ABSTAIN", 0.9)));
    let (mut client, repo) = stand.workspace().await;
    let legacy = Planned::bare(&mut client, &repo, level).await;
    stand.set_mode(&legacy.target.program_id, None).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = legacy.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, route(level), params.clone()).await;
    assert_eq!(
        recorded["advice"],
        json!({"status":"unavailable","reason":"mode_not_set",
            "message":"Jev advice is not available: the Program has no mode; \
                set it with program.save"})
    );
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(recorded["candidate_set"]["status"], "review_required");
    assert_eq!(legacy.recorded(&stand).await.last().unwrap(), FIRST);

    // One journal row for the point that fired, with the work it was for and no call.
    assert_eq!(
        journalled(&stand, &recorded["advice"]).await,
        (json!(false), json!("not_sent"), json!("mode_not_set"))
    );
    let row = stand.newest_row_of("intake").await;
    assert_eq!(row["request"], Value::Null);
    assert_eq!(row["route"], route(level));
    assert_eq!(row["request_id"], json!(request_id));
    assert_eq!(row["program_id"], legacy.target.program_id);
    assert_eq!(row["candidate_set_id"], legacy.set()["id"]);
    assert_eq!(row["scope_id"], legacy.scope_id());
    let counters = audit(&mut client).await["counters"].clone();
    assert_eq!(
        (&counters["called"], &counters["not_called"]),
        (&json!(0), &json!(1))
    );
    assert_eq!(counters["reasons"], json!({"mode_not_set":1}));

    // A replay returns the same reply, also once the Program has a mode: no call, no row.
    let (replayed, _) = command(&mut client, route(level), params.clone()).await;
    assert_eq!(replayed, recorded);
    stand.set_mode(&legacy.target.program_id, Some("mvp")).await;
    let (replayed, _) = command(&mut client, route(level), params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.rows_of("intake").await, (1, 0));
    assert_eq!(stand.count("advice_journal").await, 2);

    // Disabled is decided first: without a mode and without Jev the input is recorded
    // as `main` records it, with no field and no row.
    let (mut other, repo) = stand.workspace().await;
    let off = Planned::bare(&mut other, &repo, level).await;
    stand.set_mode(&off.target.program_id, None).await;
    let params = off.input(FIRST, Uuid::new_v4());
    let (plain, _) = command(&mut other, route(level), params).await;
    assert!(plain.get("advice").is_none(), "{plain}");
    assert_eq!(stand.count("advice_journal").await, 2);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
    other.finish().await;
}

/// A Program whose text was erased has no statement to send. On the Slice level the
/// command does not read the Program itself: the input is recorded all the same and
/// the reply says why there is no advice.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_of_an_erased_program_is_recorded_and_not_sent_to_jev() {
    let level = Level::Slices;
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(|_| ("ABSTAIN", 0.9)));
    let (mut client, repo) = stand.workspace().await;
    let erased = Planned::bare(&mut client, &repo, level).await;
    let program = Uuid::parse_str(erased.target.program_id.as_str().unwrap()).unwrap();
    let marked =
        sqlx::query("UPDATE programs SET payload_erased=true WHERE tenant_id=$1 AND id=$2")
            .bind(stand.tenant)
            .bind(program)
            .execute(&stand.pool)
            .await
            .unwrap();
    assert_eq!(marked.rows_affected(), 1);
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = erased.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, route(level), params.clone()).await;
    assert_eq!(
        recorded["advice"],
        json!({"status":"unavailable","reason":"program_unavailable",
            "message":"Jev advice is not available: the Program's text is erased or \
                cannot be read"})
    );
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(erased.recorded(&stand).await.last().unwrap(), FIRST);
    assert_eq!(
        journalled(&stand, &recorded["advice"]).await,
        (
            json!(false),
            json!("not_sent"),
            json!("program_unavailable")
        )
    );
    let row = stand.newest_row_of("intake").await;
    assert_eq!(row["request"], Value::Null);
    assert_eq!(row["request_id"], json!(request_id));
    assert_eq!(row["program_id"], erased.target.program_id);
    assert_eq!(row["scope_id"], erased.scope_id());

    // A replay returns the same reply: no call, no further row, no second input.
    let (replayed, _) = command(&mut client, route(level), params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.rows_of("intake").await, (1, 0));
    assert_eq!(stand.count("advice_journal").await, 2);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_is_recorded_when_jev_times_out_or_answers_badly() {
    a_failing_jev_leaves_the_input_recorded(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_is_recorded_when_jev_times_out_or_answers_badly() {
    a_failing_jev_leaves_the_input_recorded(Level::Slices).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_is_recorded_when_the_daemon_has_no_provider() {
    a_daemon_without_a_provider_records_the_input_and_says_so(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_is_recorded_when_the_daemon_has_no_provider() {
    a_daemon_without_a_provider_records_the_input_and_says_so(Level::Slices).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_of_a_program_without_a_mode_is_not_sent_to_jev() {
    an_input_of_a_program_without_a_mode_is_recorded_without_a_call(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_of_a_program_without_a_mode_is_not_sent_to_jev() {
    an_input_of_a_program_without_a_mode_is_recorded_without_a_call(Level::Slices).await;
}
