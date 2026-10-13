//! One real call of the `intake` point to the Jev service per level, beside the real
//! `model` call of the same input. Ignored by default: the calls spend money and need
//! the key in `TECT_TEST_LIVE_JEV_KEY`. Run it alone and read what it prints:
//!
//! `cargo test -p tect-cli --test advice_intake live -- --ignored --nocapture`
use crate::drafts::Level;
use crate::inputs::{Planned, route};
use crate::support::{Stand, command, configure};
use uuid::Uuid;

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

/// An input a person could have written after the fixture's request and its draft:
/// one small addition to the diagnosis, and work far beyond the Program beside it.
fn input(level: Level) -> &'static str {
    match level {
        Level::Scopes => {
            "Also check the preview in the mobile app, and while you are there move the \
             whole notification service to a new message queue."
        }
        Level::Slices => {
            "Before the diagnosis, record which saved setting the preview ignores; \
             and add a load test for the whole rendering service."
        }
    }
}

async fn the_real_jev_answers_one_input(level: Level) {
    let key = std::env::var("TECT_TEST_LIVE_JEV_KEY").expect("TECT_TEST_LIVE_JEV_KEY required");
    let stand = Stand::start_at(Some(ENDPOINT), &key, "30000").await;
    let (mut client, repo) = stand.workspace().await;
    // The draft is saved before Jev is enabled: the calls are those of the recorded input.
    let mut planned = Planned::bare(&mut client, &repo, level).await;
    let save = planned
        .target
        .save_draft(planned.target.overgrown(), Uuid::new_v4());
    planned.save(&mut client, save).await;
    configure(&mut client, true, 0).await;

    let params = planned.input(input(level), Uuid::new_v4());
    let (recorded, envelope) = command(&mut client, route(level), params).await;
    let row = stand.newest_row_of("intake").await;
    let answer: Option<String> = sqlx::query_scalar(
        "SELECT pg_catalog.convert_from(response_body, 'UTF8') FROM advice_journal \
         WHERE tenant_id=$1 AND point='intake' ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(stand.tenant)
    .fetch_one(&stand.pool)
    .await
    .unwrap();
    assert!(!recorded.to_string().contains(&key) && !row.to_string().contains(&key));
    assert!(answer.as_ref().is_none_or(|body| !body.contains(&key)));

    println!("level={} reply_envelope_bytes={envelope}", level.name());
    println!("advice={}", recorded["advice"]);
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
        recorded["advice"]["status"], "advised",
        "{}",
        recorded["advice"]
    );
    assert_eq!(row["advice"], recorded["advice"]);
    assert_eq!(row["point"], "intake");
    assert_eq!(stand.rows_of("intake").await, (1, 1));
    // The second point of the command wrote the other row.
    assert_eq!(stand.count("advice_journal").await, 2);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "calls the real Jev service; needs TECT_TEST_LIVE_JEV_KEY"]
async fn live_scope_input_gets_advice_from_the_real_jev() {
    the_real_jev_answers_one_input(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "calls the real Jev service; needs TECT_TEST_LIVE_JEV_KEY"]
async fn live_slice_input_gets_advice_from_the_real_jev() {
    the_real_jev_answers_one_input(Level::Slices).await;
}
