//! One real call of the `model` point to the Jev service per level, beside the real
//! `intake` call of the same input. Ignored by default: the calls spend money and need
//! the key in `TECT_TEST_LIVE_JEV_KEY`. Run it alone and read what it prints:
//!
//! `cargo test -p tect-cli --test advice_model live -- --ignored --nocapture`
use crate::drafts::Level;
use crate::inputs::{Planned, route};
use crate::model::both_rows;
use crate::support::{Stand, command, configure};
use serde_json::json;
use uuid::Uuid;

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// An input a person could have written after the fixture's request and its draft:
/// one narrow practical change, and beside it intellectual work of its own.
fn input(level: Level) -> &'static str {
    match level {
        Level::Scopes => {
            "Rename the preview flag in the settings file to preview_enabled. And before \
             the correction is chosen, compare the three rendering libraries we could move \
             to and recommend one."
        }
        Level::Slices => {
            "Add the saved setting the preview ignores to the reproduction notes. Then \
             review the whole rendering path for other settings it may ignore and write \
             down what you find."
        }
    }
}

async fn the_real_jev_advises_on_the_executors_of_one_input(level: Level) {
    let key = std::env::var("TECT_TEST_LIVE_JEV_KEY").expect("TECT_TEST_LIVE_JEV_KEY required");
    let stand = Stand::start_at(Some(ENDPOINT), &key, "20000").await;
    let (mut client, repo) = stand.workspace().await;
    // The draft is saved before Jev is enabled: the calls are those of the recorded input.
    let mut planned = Planned::bare(&mut client, &repo, level).await;
    let save = planned
        .target
        .save_draft(planned.target.overgrown(), Uuid::new_v4());
    planned.save(&mut client, save).await;
    configure(&mut client, true, 0).await;

    let request_id = Uuid::new_v4();
    let params = planned.input(input(level), request_id);
    let (recorded, envelope) = command(&mut client, route(level), params).await;
    let (intake, row) = both_rows(&stand, request_id).await;
    let answer: Option<String> = sqlx::query_scalar(
        "SELECT pg_catalog.convert_from(response_body, 'UTF8') FROM advice_journal \
         WHERE tenant_id=$1 AND point='model' ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(stand.tenant)
    .fetch_one(&stand.pool)
    .await
    .unwrap();
    let written = json!([intake, row]).to_string();
    assert!(!recorded.to_string().contains(&key) && !written.contains(&key));
    assert!(answer.as_ref().is_none_or(|body| !body.contains(&key)));

    println!("level={} reply_envelope_bytes={envelope}", level.name());
    println!("intake_advice={}", recorded["advice"]);
    println!("model_advice={}", recorded["model_advice"]);
    for fact in [
        "outcome",
        "reason",
        "send_certainty",
        "model",
        "request_bytes",
        "response_bytes",
        "input_tokens",
        "output_tokens",
        "latency_ms",
    ] {
        println!("{fact}={}", row[fact]);
    }
    println!("answer_body={}", answer.unwrap_or_default());

    assert_eq!(
        recorded["model_advice"]["status"], "advised",
        "{}",
        recorded["model_advice"]
    );
    assert_eq!(row["advice"], recorded["model_advice"]);
    assert_eq!(row["request"]["state"], intake["request"]["state"]);
    assert_eq!(stand.count("advice_journal").await, 2);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "calls the real Jev service; needs TECT_TEST_LIVE_JEV_KEY"]
async fn live_scope_input_gets_model_advice_from_the_real_jev() {
    the_real_jev_advises_on_the_executors_of_one_input(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "calls the real Jev service; needs TECT_TEST_LIVE_JEV_KEY"]
async fn live_slice_input_gets_model_advice_from_the_real_jev() {
    the_real_jev_advises_on_the_executors_of_one_input(Level::Slices).await;
}
