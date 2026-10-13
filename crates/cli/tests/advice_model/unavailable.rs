//! One point without advice does not touch the other, and the input is recorded
//! either way; and what a Program without a mode, a Program whose text cannot be read
//! and a daemon without a provider give both points.
use crate::drafts::Level;
use crate::inputs::{FIRST, Planned, route};
use crate::model::{
    NOT_AN_ANSWER, advised, both_rows, intake_advised, journalled, settled, unavailable,
};
use crate::support::{BUDGET, Jev, Stand, command, configure};
use serde_json::{Value, json};
use uuid::Uuid;

/// Both points fired without a call: each row says so, for the same work, with the
/// field its point gave.
async fn neither_called(stand: &Stand, request_id: Uuid, recorded: &Value, reason: &str) {
    let (intake, model) = both_rows(stand, request_id).await;
    assert_eq!(intake["advice"], recorded["advice"]);
    assert_eq!(model["advice"], recorded["model_advice"]);
    for row in [&intake, &model] {
        assert_eq!(
            journalled(row),
            (
                &json!(false),
                &json!("not_sent"),
                &json!("unavailable"),
                &json!(reason)
            ),
            "{}",
            row["point"]
        );
        assert_eq!(row["request"], Value::Null);
    }
    for work in ["route", "request_id", "program_id", "candidate_set_id"] {
        assert!(!model[work].is_null(), "{work}");
        assert_eq!(model[work], intake[work], "{work}");
    }
    assert_eq!(model["scope_id"], intake["scope_id"]);
}

async fn one_point_failing_leaves_the_other_advised_and_the_input_recorded(level: Level) {
    let stand = Stand::start("2000").await;
    let not_an_answer = json!({"status":"unavailable","reason":"invalid_response",
        "message":NOT_AN_ANSWER});
    let failed = (
        &json!(true),
        &json!("sent"),
        &json!("unavailable"),
        &json!("invalid_response"),
    );
    let answered = (
        &json!(true),
        &json!("sent"),
        &json!("advised"),
        &Value::Null,
    );

    // Jev answers `intake` and fails `model`.
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::bare(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::AnswersBut(settled, "practical"));
    let request_id = Uuid::new_v4();
    let params = planned.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, route(level), params).await;
    assert_eq!(recorded["advice"], intake_advised(level, false));
    assert_eq!(recorded["model_advice"], not_an_answer, "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(recorded["candidate_set"]["status"], "review_required");
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    let (intake, model) = both_rows(&stand, request_id).await;
    assert_eq!(model["advice"], recorded["model_advice"]);
    assert_eq!(
        (journalled(&intake), journalled(&model)),
        (answered, failed)
    );

    // The other way round in another workspace: `intake` fails, `model` is advised.
    let (mut other, repo) = stand.workspace().await;
    let planned = Planned::bare(&mut other, &repo, level).await;
    configure(&mut other, true, 0).await;
    stand.jev.answer(Jev::AnswersBut(settled, "fit"));
    let request_id = Uuid::new_v4();
    let params = planned.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut other, route(level), params).await;
    assert_eq!(recorded["advice"], not_an_answer, "{recorded}");
    assert_eq!(recorded["model_advice"], advised(), "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    let (intake, model) = both_rows(&stand, request_id).await;
    assert_eq!(intake["advice"], recorded["advice"]);
    assert_eq!(model["advice"], recorded["model_advice"]);
    assert_eq!(
        (journalled(&intake), journalled(&model)),
        (failed, answered)
    );
    // Every point asked once in each workspace, whatever the other got.
    assert_eq!(stand.jev.calls(), 4);
    assert_eq!(stand.jev.asked("practical").len(), 2);
    client.finish().await;
    other.finish().await;
}

/// A Program opened before the product level existed has none, and the state both
/// points send carries the level: neither asks Jev until `program.save` states one.
async fn a_program_without_a_mode_gets_mode_not_set_from_both_points(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let legacy = Planned::bare(&mut client, &repo, level).await;
    stand.set_mode(&legacy.target.program_id, None).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = legacy.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, route(level), params.clone()).await;
    let without_mode = unavailable(
        "mode_not_set",
        "the Program has no mode; set it with program.save",
    );
    assert_eq!(recorded["model_advice"], without_mode, "{recorded}");
    assert_eq!(recorded["advice"], without_mode, "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(recorded["candidate_set"]["status"], "review_required");
    assert_eq!(legacy.recorded(&stand).await.last().unwrap(), FIRST);
    neither_called(&stand, request_id, &recorded, "mode_not_set").await;
    let (_, model) = both_rows(&stand, request_id).await;
    assert_eq!(model["program_id"], legacy.target.program_id);
    assert_eq!(model["candidate_set_id"], legacy.set()["id"]);
    assert_eq!(model["scope_id"], legacy.scope_id());

    // A replay returns the same reply, also once the Program has a mode: no call, no row.
    let (replayed, _) = command(&mut client, route(level), params.clone()).await;
    assert_eq!(replayed, recorded);
    stand.set_mode(&legacy.target.program_id, Some("mvp")).await;
    let (replayed, _) = command(&mut client, route(level), params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.count("advice_journal").await, 2);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}

/// The workspace enabled Jev and the daemon was started without a provider.
async fn a_daemon_without_a_provider_says_so_for_both_points(level: Level) {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::bare(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = planned.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, route(level), params.clone()).await;
    let no_provider = unavailable(
        "provider_unconfigured",
        "the daemon has no Jev provider configured",
    );
    assert_eq!(recorded["model_advice"], no_provider, "{recorded}");
    assert_eq!(recorded["advice"], no_provider, "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    neither_called(&stand, request_id, &recorded, "provider_unconfigured").await;
    let (replayed, _) = command(&mut client, route(level), params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.count("advice_journal").await, 2);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}

/// A Program whose text was erased has no statement to send, and the state of both
/// points holds the statement. On the Slice level the command does not read the
/// Program itself: the input is recorded all the same and each reply field says why
/// there is no advice.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_of_an_erased_program_reaches_jev_from_neither_point() {
    let level = Level::Slices;
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
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
    let no_program = unavailable(
        "program_unavailable",
        "the Program's text is erased or cannot be read",
    );
    assert_eq!(recorded["model_advice"], no_program, "{recorded}");
    assert_eq!(recorded["advice"], no_program, "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(erased.recorded(&stand).await.last().unwrap(), FIRST);
    neither_called(&stand, request_id, &recorded, "program_unavailable").await;

    // A replay returns the same reply: no call, no further row, no second input.
    let (replayed, _) = command(&mut client, route(level), params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.count("advice_journal").await, 2);
    assert_eq!(stand.jev.calls(), 0);
    let inputs = erased.recorded(&stand).await;
    assert_eq!(inputs.iter().filter(|text| *text == FIRST).count(), 1);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_keeps_the_advice_of_one_point_when_jev_fails_the_other() {
    one_point_failing_leaves_the_other_advised_and_the_input_recorded(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_keeps_the_advice_of_one_point_when_jev_fails_the_other() {
    one_point_failing_leaves_the_other_advised_and_the_input_recorded(Level::Slices).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_of_a_program_without_a_mode_reaches_jev_from_neither_point() {
    a_program_without_a_mode_gets_mode_not_set_from_both_points(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_of_a_program_without_a_mode_reaches_jev_from_neither_point() {
    a_program_without_a_mode_gets_mode_not_set_from_both_points(Level::Slices).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_gets_no_provider_from_both_points_of_a_daemon_without_one() {
    a_daemon_without_a_provider_says_so_for_both_points(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_gets_no_provider_from_both_points_of_a_daemon_without_one() {
    a_daemon_without_a_provider_says_so_for_both_points(Level::Slices).await;
}
