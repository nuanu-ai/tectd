//! The daily call limit and the room of the reply: each point is counted and measured
//! for itself.
use crate::drafts::{Level, Shape};
use crate::inputs::{FIRST, Planned, SECOND, route};
use crate::model::{advised, both_rows, enable, intake_advised, journalled, settled, unavailable};
use crate::support::{BUDGET, Jev, Stand, command, configure};
use serde_json::{Value, json};
use tect_domain::{INTAKE_NOTE, MODEL_NOTE, ModelAdvice};
use uuid::Uuid;

async fn the_daily_limit_reached_between_the_two_points_stops_the_second(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(settled));
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::bare(&mut client, &repo, level).await;
    // One call a day: `intake` takes it.
    enable(&mut client, 1, 0).await;
    let request_id = Uuid::new_v4();
    let params = planned.input(FIRST, request_id);
    let (recorded, _) = command(&mut client, route(level), params).await;
    assert_eq!(recorded["advice"], intake_advised(level, false));
    let reached = |limit: u8| {
        unavailable(
            "daily_limit_reached",
            &format!("the workspace daily call limit of {limit} is reached"),
        )
    };
    assert_eq!(recorded["model_advice"], reached(1), "{recorded}");
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    let (intake, model) = both_rows(&stand, request_id).await;
    assert_eq!(model["advice"], recorded["model_advice"]);
    assert_eq!(intake["called"], true);
    assert_eq!(
        journalled(&model),
        (
            &json!(false),
            &json!("not_sent"),
            &json!("unavailable"),
            &json!("daily_limit_reached")
        )
    );
    assert_eq!(model["request"], Value::Null);
    assert_eq!(stand.jev.calls(), 1);
    assert!(stand.jev.asked("practical").is_empty());

    // Each call counts on its own: two calls a day are those of one input, and the
    // next input gets advice from neither point.
    let (mut other, repo) = stand.workspace().await;
    let mut planned = Planned::bare(&mut other, &repo, level).await;
    enable(&mut other, 2, 0).await;
    let params = planned.input(FIRST, Uuid::new_v4());
    let (recorded, _) = command(&mut other, route(level), params).await;
    assert_eq!(recorded["advice"], intake_advised(level, false));
    assert_eq!(recorded["model_advice"], advised());
    assert_eq!(stand.jev.calls(), 3);
    planned.follow(&recorded);
    let request_id = Uuid::new_v4();
    let params = planned.input(SECOND, request_id);
    let (recorded, _) = command(&mut other, route(level), params).await;
    assert_eq!(recorded["advice"], reached(2), "{recorded}");
    assert_eq!(recorded["model_advice"], reached(2), "{recorded}");
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), SECOND);
    let (intake, model) = both_rows(&stand, request_id).await;
    assert_eq!(
        (&intake["called"], &model["called"]),
        (&json!(false), &json!(false))
    );
    assert_eq!(stand.jev.calls(), 3);
    client.finish().await;
    other.finish().await;
}

/// Every answer of both points at its longest token and widest probability.
fn longest(name: &str) -> (&'static str, f64) {
    match name {
        "work" => ("oversized", 0.99),
        "placement" => ("outside_program", 0.99),
        // A position in the draft prints shorter than this.
        "nearest" => ("ABSTAIN", 0.99),
        "fit" => ("beyond_mode", 0.99),
        "practical" => ("implementation", 0.99),
        "specialist" => ("demanding", 0.99),
        other => panic!("unexpected question {other}"),
    }
}

/// Bytes a field costs inside a reply, where the data travels as a JSON string.
fn reply_bytes(field: &Value) -> usize {
    serde_json::to_string(&field.to_string()).unwrap().len()
}

/// `intake` is measured on a reply that keeps room for the short field of the second
/// point, so the largest field `intake` can give never costs the second point its
/// answer. On the replies of these two commands it leaves room for more than that:
/// the largest field of the second point.
async fn an_intake_field_at_its_largest_leaves_the_second_point_its_room(level: Level) {
    let stand = Stand::start("2000").await;
    stand.jev.answer(Jev::Answers(longest));
    let (mut client, repo) = stand.workspace().await;
    let planned = Planned::drafted(&mut client, &repo, level, &Shape::of(2, 2)).await;
    configure(&mut client, true, 0).await;
    let request_id = Uuid::new_v4();
    let params = planned.input(FIRST, request_id);
    let (recorded, envelope) = command(&mut client, route(level), params).await;
    assert_eq!(
        recorded["advice"],
        json!({"status":"advised","note":INTAKE_NOTE,"level":level.name(),
            "work":"oversized","work_p":0.99,"placement":"outside_program","placement_p":0.99,
            "nearest":"ABSTAIN","nearest_p":0.99,"fit":"beyond_mode","fit_p":0.99}),
        "{recorded}"
    );
    assert_eq!(
        recorded["model_advice"],
        json!({"status":"advised","note":MODEL_NOTE,
            "practical":"implementation","practical_p":0.99,
            "specialist":"demanding","specialist_p":0.99}),
        "{recorded}"
    );
    assert!(envelope <= BUDGET, "{envelope}");
    // The field the point would have given without room is shorter than the one it gave.
    let short = json!(ModelAdvice::without_room());
    assert_eq!(short["reason"], "advice_too_large");
    assert!(reply_bytes(&short) < reply_bytes(&recorded["model_advice"]));
    println!(
        "{}: reply envelope bytes with both fields at their largest={envelope}, of {BUDGET}; \
         largest model_advice bytes={}, its advice_too_large field bytes={}",
        route(level),
        reply_bytes(&recorded["model_advice"]),
        reply_bytes(&short)
    );
    let (intake, model) = both_rows(&stand, request_id).await;
    assert_eq!(intake["advice"], recorded["advice"]);
    assert_eq!(model["advice"], recorded["model_advice"]);
    assert_eq!(planned.recorded(&stand).await.last().unwrap(), FIRST);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_gets_no_model_advice_once_intake_took_the_last_call_of_the_day() {
    the_daily_limit_reached_between_the_two_points_stops_the_second(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_gets_no_model_advice_once_intake_took_the_last_call_of_the_day() {
    the_daily_limit_reached_between_the_two_points_stops_the_second(Level::Slices).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn scope_input_with_the_largest_intake_field_still_carries_model_advice() {
    an_intake_field_at_its_largest_leaves_the_second_point_its_room(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn slice_input_with_the_largest_intake_field_still_carries_model_advice() {
    an_intake_field_at_its_largest_leaves_the_second_point_its_room(Level::Slices).await;
}
