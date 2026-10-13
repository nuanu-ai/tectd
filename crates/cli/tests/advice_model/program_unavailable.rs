//! What the one point of `program.record_input` gives when it has no advice: a Jev
//! that fails, a Program without a mode, a daemon without a provider, the daily call
//! limit and the ceiling of a request, each with the input recorded. And a Program
//! whose text is erased, where the command itself is refused and the point does not
//! fire.
use crate::drafts::PROGRAM_WORDS;
use crate::inputs::{FIRST, SECOND};
use crate::model::{NOT_AN_ANSWER, advised, enable, journalled, settled, unavailable};
use crate::program_input::{ROUTE, Stated, state, the_row};
use crate::support::{BUDGET, Jev, Stand, command, configure};
use serde_json::{Value, json};
use std::time::Duration;
use tect_domain::ProgramMode;
use uuid::Uuid;

/// The point fired without a call: the row says so, for the Program, with the field
/// the agent received.
async fn not_called(stand: &Stand, request_id: Uuid, recorded: &Value, reason: &str) -> Value {
    let row = the_row(stand, request_id).await;
    assert_eq!(row["advice"], recorded["model_advice"]);
    assert_eq!(
        journalled(&row),
        (
            &json!(false),
            &json!("not_sent"),
            &json!("unavailable"),
            &json!(reason)
        )
    );
    assert_eq!(row["request"], Value::Null);
    assert_eq!(row["route"], ROUTE);
    assert_eq!(row["request_id"], json!(request_id));
    row
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_input_stays_recorded_when_jev_fails() {
    let stand = Stand::start("800").await;

    // Jev answers after the daemon has stopped waiting.
    let (mut client, repo) = stand.workspace().await;
    let late = Stated::ready(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    stand.jev.answer(Jev::Late(Duration::from_millis(2500)));
    let request_id = Uuid::new_v4();
    let params = late.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, ROUTE, params.clone()).await;
    assert_eq!(
        recorded["model_advice"],
        unavailable("timeout", "Jev did not answer within 800 ms"),
        "{recorded}"
    );
    assert!(recorded.get("advice").is_none(), "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(recorded["program"]["current_step"], "compose");
    assert_eq!(recorded["program"]["latest_input"], 2);
    assert_eq!(late.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);
    let row = the_row(&stand, request_id).await;
    assert_eq!(row["advice"], recorded["model_advice"]);
    assert_eq!(
        journalled(&row),
        (
            &json!(true),
            &json!("sent_unknown"),
            &json!("unavailable"),
            &json!("timeout")
        )
    );
    // The request left whole, over the state of this input.
    assert_eq!(
        row["request"]["state"],
        state(ProgramMode::Prod, &[PROGRAM_WORDS], FIRST)
    );
    // The replay returns the recorded outcome and does not ask again.
    let (replayed, _) = command(&mut client, ROUTE, params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.jev.calls(), 1);
    assert_eq!(stand.count("advice_journal").await, 1);
    assert_eq!(late.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);

    // Jev answers in time with something that is not an answer.
    let (mut other, repo) = stand.workspace().await;
    let bad = Stated::ready(&mut other, &repo).await;
    configure(&mut other, true, 0).await;
    stand.jev.answer(Jev::Garbage);
    let request_id = Uuid::new_v4();
    let (recorded, _) = command(&mut other, ROUTE, bad.input(FIRST, request_id)).await;
    assert_eq!(
        recorded["model_advice"],
        json!({"status":"unavailable","reason":"invalid_response","message":NOT_AN_ANSWER}),
        "{recorded}"
    );
    assert_eq!(bad.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);
    let row = the_row(&stand, request_id).await;
    assert_eq!(row["advice"], recorded["model_advice"]);
    assert_eq!(
        journalled(&row),
        (
            &json!(true),
            &json!("sent"),
            &json!("unavailable"),
            &json!("invalid_response")
        )
    );
    // The failure is of that call alone: the next input is advised.
    stand.jev.answer(Jev::Answers(settled));
    let (next, _) = command(&mut other, ROUTE, bad.input(SECOND, Uuid::new_v4())).await;
    assert_eq!(next["model_advice"], advised());
    assert_eq!(stand.jev.calls(), 3);
    client.finish().await;
    other.finish().await;
}

/// The state carries the product level, and a Program has none until `program.save`
/// states one: a Program opened before the level existed, and a Program just begun.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_input_of_a_program_without_a_mode_gets_mode_not_set() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let without_mode = unavailable(
        "mode_not_set",
        "the Program has no mode; set it with program.save",
    );

    // Stated in full, and without a mode.
    let (mut client, repo) = stand.workspace().await;
    let legacy = Stated::ready(&mut client, &repo).await;
    stand.set_mode(&legacy.id, None).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = legacy.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, ROUTE, params.clone()).await;
    assert_eq!(recorded["model_advice"], without_mode, "{recorded}");
    assert!(recorded.get("advice").is_none(), "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(legacy.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);
    let row = not_called(&stand, request_id, &recorded, "mode_not_set").await;
    assert_eq!(row["program_id"], legacy.id);

    // A replay returns the same reply: no call, no row. Once the Program has a mode
    // the replay still carries the field stored for the request, beside the Program
    // as it now stands: the command answers a replay with the current Program.
    let (replayed, _) = command(&mut client, ROUTE, params.clone()).await;
    assert_eq!(replayed, recorded);
    stand.set_mode(&legacy.id, Some("mvp")).await;
    let (replayed, _) = command(&mut client, ROUTE, params).await;
    assert_eq!(replayed["model_advice"], recorded["model_advice"]);
    assert_eq!(replayed["program"], recorded["program"]);
    assert_eq!(stand.count("advice_journal").await, 1);
    assert_eq!(stand.jev.calls(), 0);
    // The next input is judged at the level the Program now has.
    let (next, _) = command(&mut client, ROUTE, legacy.input(SECOND, Uuid::new_v4())).await;
    assert_eq!(next["model_advice"], advised());
    assert_eq!(
        stand.jev.requests()[0]["state"]["mode"]["level"],
        ProgramMode::Mvp.as_str()
    );

    // A Program just begun and never saved: nothing of it is stated, the mode
    // neither. Each further input is recorded and gets the same answer.
    let (mut other, repo) = stand.workspace().await;
    other.call("open_workspace", json!({})).await;
    let registered = other.call("register_source", json!({"path":repo})).await;
    other
        .call(
            "select_worktrees",
            json!({"worktree_ids":[registered["id"]]}),
        )
        .await;
    let begun = other
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":PROGRAM_WORDS}),
        )
        .await;
    let draft = Stated {
        id: begun["program"]["id"].clone(),
    };
    configure(&mut other, true, 0).await;
    for (text, latest) in [(FIRST, 2), (SECOND, 3)] {
        let request_id = Uuid::new_v4();
        let (recorded, _) = command(&mut other, ROUTE, draft.input(text, request_id)).await;
        assert_eq!(recorded["model_advice"], without_mode, "{recorded}");
        assert_eq!(recorded["program"]["status"], "draft");
        assert_eq!(recorded["program"]["latest_input"], latest);
        not_called(&stand, request_id, &recorded, "mode_not_set").await;
    }
    assert_eq!(draft.recorded(&stand).await, [PROGRAM_WORDS, FIRST, SECOND]);
    assert_eq!(stand.jev.calls(), 1);
    client.finish().await;
    other.finish().await;
}

/// The workspace enabled Jev and the daemon was started without a provider.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_input_gets_no_provider_from_a_daemon_without_one() {
    let stand = Stand::start_without_provider().await;
    let (mut client, repo) = stand.workspace().await;
    let program = Stated::ready(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = program.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, ROUTE, params.clone()).await;
    assert_eq!(
        recorded["model_advice"],
        unavailable(
            "provider_unconfigured",
            "the daemon has no Jev provider configured"
        ),
        "{recorded}"
    );
    assert!(recorded.get("advice").is_none(), "{recorded}");
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(program.recorded(&stand).await, [PROGRAM_WORDS, FIRST]);
    not_called(&stand, request_id, &recorded, "provider_unconfigured").await;
    let (replayed, _) = command(&mut client, ROUTE, params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.count("advice_journal").await, 1);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_input_gets_no_model_advice_once_the_daily_limit_is_reached() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let program = Stated::ready(&mut client, &repo).await;
    // One call a day: the first input takes it. The command has one point, so one
    // input costs one call.
    enable(&mut client, 1, 0).await;
    let (first, _) = command(&mut client, ROUTE, program.input(FIRST, Uuid::new_v4())).await;
    assert_eq!(first["model_advice"], advised());
    assert_eq!(stand.jev.calls(), 1);

    let request_id = Uuid::new_v4();
    let params = program.input(SECOND, request_id);
    let (recorded, _) = command(&mut client, ROUTE, params.clone()).await;
    assert_eq!(
        recorded["model_advice"],
        unavailable(
            "daily_limit_reached",
            "the workspace daily call limit of 1 is reached"
        ),
        "{recorded}"
    );
    assert_eq!(
        program.recorded(&stand).await,
        [PROGRAM_WORDS, FIRST, SECOND]
    );
    not_called(&stand, request_id, &recorded, "daily_limit_reached").await;
    // A replay returns the recorded field; it is not counted and asks nobody.
    let (replayed, _) = command(&mut client, ROUTE, params).await;
    assert_eq!(replayed, recorded);
    assert_eq!(stand.jev.calls(), 1);
    assert_eq!(stand.count("advice_journal").await, 2);
    client.finish().await;
}

/// The state holds the new input, and this one alone is over the 256 KiB a request to
/// Jev may take: the input is recorded and not sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_input_over_the_request_ceiling_is_recorded_and_not_sent_to_jev() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let program = Stated::ready(&mut client, &repo).await;
    configure(&mut client, true, 0).await;
    let long = "A long amendment. ".repeat(16_000);
    let request_id = Uuid::new_v4();
    let (recorded, envelope) = command(&mut client, ROUTE, program.input(&long, request_id)).await;
    let field = &recorded["model_advice"];
    assert_eq!(field["status"], "unavailable", "{field}");
    assert_eq!(field["reason"], "request_too_large", "{field}");
    let message = field["message"].as_str().unwrap();
    assert!(
        message.starts_with("Jev advice is not available: the request of ")
            && message.ends_with(" bytes exceeds the limit of 262144 bytes"),
        "{message}"
    );
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(program.recorded(&stand).await.last().unwrap(), &long);
    not_called(&stand, request_id, &recorded, "request_too_large").await;
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}

/// A Program whose text was erased has no statement to send. On the Program level the
/// command reads the Program itself, so it is refused before anything is recorded, as
/// it is without the feature, and the point does not fire. `program_unavailable` is
/// left for a Program whose text goes between the command's commit and the reading
/// of the point.
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn program_input_of_an_erased_program_is_refused_before_the_point_fires() {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let erased = Stated::ready(&mut client, &repo).await;
    let program = Uuid::parse_str(erased.id.as_str().unwrap()).unwrap();
    let marked =
        sqlx::query("UPDATE programs SET payload_erased=true WHERE tenant_id=$1 AND id=$2")
            .bind(stand.tenant)
            .bind(program)
            .execute(&stand.pool)
            .await
            .unwrap();
    assert_eq!(marked.rows_affected(), 1);
    configure(&mut client, true, 0).await;
    let refused = client
        .call_error("record_program_input", erased.input(FIRST, Uuid::new_v4()))
        .await;
    assert_eq!(refused["error"]["code"], "knowledge_payload_erased");
    assert_eq!(refused["error"]["route"], ROUTE);
    assert!(refused.get("model_advice").is_none(), "{refused}");
    assert_eq!(erased.recorded(&stand).await, [PROGRAM_WORDS]);
    assert_eq!(stand.count("advice_journal").await, 0);
    assert_eq!(stand.jev.calls(), 0);
    client.finish().await;
}
