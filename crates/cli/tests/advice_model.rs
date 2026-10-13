//! The `model` advice point on `scope.candidates.record_input` and
//! `slice.candidates.input`, a second point on those commands after `intake`, and on
//! `program.record_input`, where it is the only point, through a real daemon and MCP
//! bridge against PostgreSQL. Jev is a loopback fake; nothing leaves the machine.
#[path = "advice_model/disabled.rs"]
mod disabled;
#[path = "advice_sizing/drafts.rs"]
#[allow(dead_code)]
mod drafts;
#[path = "advice_intake/inputs.rs"]
#[allow(dead_code)]
mod inputs;
#[path = "advice_model/limits.rs"]
mod limits;
#[path = "advice_model/live.rs"]
mod live;
#[path = "advice_model/support.rs"]
mod model;
#[path = "advice_pipeline/support.rs"]
#[allow(dead_code)]
mod pipeline;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod planning;
#[path = "advice_model/program_input.rs"]
mod program_input;
#[path = "advice_model/program_unavailable.rs"]
mod program_unavailable;
#[allow(dead_code)]
mod recovery_support;
#[path = "advice_model/result_inputs.rs"]
mod result_inputs;
#[path = "advice_sizing/support.rs"]
#[allow(dead_code)]
mod support;
#[path = "advice_model/unavailable.rs"]
mod unavailable;
#[path = "mode_override/walk.rs"]
#[allow(dead_code)]
mod walk;

use drafts::{Level, Shape};
use inputs::{FIRST, Planned, SECOND, asked, criteria, route};
use model::{advised, audit, both_rows, intake_advised, journalled, settled};
use serde_json::{Value, json};
use support::{BUDGET, Jev, KEY, Stand, command, configure};
use tect_domain::{MODEL_NOTE, ProgramMode};
use uuid::Uuid;

/// Jev abstains on the practical work and finds demanding work for a specialist.
fn specialist_only(name: &str) -> (&'static str, f64) {
    match name {
        "practical" => ("ABSTAIN", 0.6),
        "specialist" => ("demanding", 0.9),
        other => settled(other),
    }
}

/// Jev abstains on both questions of `model` and answers `intake`.
fn silent_on_executors(name: &str) -> (&'static str, f64) {
    match name {
        "practical" | "specialist" => ("ABSTAIN", 0.9),
        other => settled(other),
    }
}

async fn both_points_answer_one_recorded_input(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::drafted(&mut client, &repo, level, &Shape::of(2, 2)).await;
    configure(&mut client, true, 0).await;

    let request_id = Uuid::new_v4();
    let params = planned.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, route(level), params.clone()).await;
    // Each point gave its own field; that of `intake` is what it is without `model`.
    assert_eq!(recorded["model_advice"], advised());
    assert_eq!(recorded["advice"], intake_advised(level, true));
    assert!(envelope <= BUDGET, "{envelope}");
    assert_eq!(recorded["candidate_set"]["status"], "review_required");
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    // Kinds of work and probabilities only: no text of the input, no name of a model.
    let field = recorded["model_advice"].to_string();
    for absent in ["preview", "ignores", "jev-latest", "jev-2026-10"] {
        assert!(!field.contains(absent), "{absent}");
    }

    // Jev was asked twice: by `intake`, then by `model` over the same state, whole.
    let requests = stand.jev.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(asked(&requests[0]), ["fit", "nearest", "placement", "work"]);
    assert_eq!(asked(&requests[1]), ["practical", "specialist"]);
    let state = &requests[1]["state"];
    assert_eq!(*state, requests[0]["state"]);
    assert_eq!(state.to_string(), requests[0]["state"].to_string());
    assert_eq!(state["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(
        *state,
        planned.state(
            ProgramMode::Prod,
            &planned.first_words(),
            FIRST,
            Some(state["candidates"].clone())
        )
    );
    assert_eq!(
        criteria(&requests[1], "practical"),
        ["ABSTAIN", "implementation", "none", "routine"]
    );
    assert_eq!(
        criteria(&requests[1], "specialist"),
        ["ABSTAIN", "demanding", "none", "standard"]
    );
    for question in ["practical", "specialist"] {
        let instructions = requests[1]["questions"][question]["instructions"]
            .as_str()
            .unwrap();
        assert!(
            instructions.contains("choose the more capable one"),
            "{question}"
        );
    }

    // Two journal rows for the request. That of `model` holds exactly the field the
    // agent received, the state `intake` sent and the work the input was for.
    let (intake, model) = both_rows(&stand, request_id).await;
    assert_eq!(model["advice"], recorded["model_advice"]);
    assert_eq!(
        model["advice"].to_string(),
        recorded["model_advice"].to_string()
    );
    assert_eq!(intake["advice"], recorded["advice"]);
    assert_eq!(model["request"], requests[1]);
    assert_eq!(model["request"]["state"], intake["request"]["state"]);
    assert_eq!(model["route"], route(level));
    assert_eq!(model["request_id"], json!(request_id));
    assert_eq!(model["program_id"], planned.target.program_id);
    assert_eq!(model["candidate_set_id"], planned.set()["id"]);
    assert_eq!(model["scope_id"], planned.scope_id());
    for row in [&intake, &model] {
        assert_eq!(
            journalled(row),
            (
                &json!(true),
                &json!("sent"),
                &json!("advised"),
                &Value::Null
            )
        );
    }
    assert_eq!(model["model"], "jev-2026-10");
    assert_eq!(
        (&model["input_tokens"], &model["output_tokens"]),
        (&json!(640), &json!(90))
    );
    assert!(!model.to_string().contains(KEY));
    // No review is held to this point: nothing is kept of its answers.
    assert_eq!(stand.count("advice_candidate_answers").await, 0);
    let page = audit(&mut client).await;
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["entries"][0]["advice"], recorded["model_advice"]);
    assert_eq!(
        (&page["counters"]["called"], &page["counters"]["advised"]),
        (&json!(1), &json!(1))
    );

    // A replay under the same request id returns the same reply, byte for byte: no
    // call, no row, and the input is not recorded twice.
    for _ in 0..2 {
        let (replayed, bytes) = command(&mut client, route(level), params.clone()).await;
        assert_eq!(replayed, recorded);
        assert_eq!(replayed.to_string(), recorded.to_string());
        assert_eq!(bytes, envelope);
    }
    assert_eq!(stand.jev.calls(), 2);
    assert_eq!(stand.count("advice_journal").await, 2);
    let inputs = planned.recorded(&stand).await;
    assert_eq!(inputs.iter().filter(|text| *text == FIRST).count(), 1);

    // In another workspace, for a set without a saved draft: a question Jev abstains
    // on keeps `ABSTAIN`, and `intake` is advised all the same.
    stand.jev.answer(Jev::Answers(specialist_only));
    let (mut other, repo) = stand.workspace().await;
    let mut bare = Planned::bare(&mut other, &repo, level).await;
    configure(&mut other, true, 0).await;
    let params = bare.input(FIRST, Uuid::new_v4());
    let (recorded, _) = command(&mut other, route(level), params).await;
    assert_eq!(
        recorded["model_advice"],
        json!({"status":"advised","note":MODEL_NOTE,"practical":"ABSTAIN","practical_p":0.6,
            "specialist":"demanding","specialist_p":0.9})
    );
    assert_eq!(recorded["advice"], intake_advised(level, false));
    assert!(stand.jev.requests()[3]["state"].get("candidates").is_none());

    // Jev abstains on both: no content. It is this point's outcome alone.
    stand.jev.answer(Jev::Answers(silent_on_executors));
    bare.follow(&recorded);
    let request_id = Uuid::new_v4();
    let params = bare.input(SECOND, request_id);
    let (recorded, _) = command(&mut other, route(level), params).await;
    assert_eq!(recorded["model_advice"], json!({"status":"abstained"}));
    assert_eq!(recorded["advice"], intake_advised(level, false));
    let (intake, model) = both_rows(&stand, request_id).await;
    assert_eq!(model["advice"], recorded["model_advice"]);
    assert_eq!(
        journalled(&model),
        (
            &json!(true),
            &json!("sent"),
            &json!("abstained"),
            &Value::Null
        )
    );
    assert_eq!(intake["outcome"], "advised");
    assert_eq!(stand.jev.calls(), 6);
    client.finish().await;
    other.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_carries_the_fields_of_both_points_and_the_journal_holds_both() {
    both_points_answer_one_recorded_input(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_carries_the_fields_of_both_points_and_the_journal_holds_both() {
    both_points_answer_one_recorded_input(Level::Slices).await;
}
