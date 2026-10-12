//! One real call to the Jev service per level. Ignored by default: each call spends
//! money and needs the key in `TECT_TEST_LIVE_JEV_KEY`. Run it alone and read what it
//! prints:
//!
//! `cargo test -p tect-cli --test advice_sizing live -- --ignored --nocapture`
use crate::drafts::{Level, target};
use crate::support::{Stand, command, configure};
use uuid::Uuid;

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";

async fn the_real_jev_answers_one_draft(level: Level) {
    let key = std::env::var("TECT_TEST_LIVE_JEV_KEY").expect("TECT_TEST_LIVE_JEV_KEY required");
    let stand = Stand::start_at(Some(ENDPOINT), &key, "30000").await;
    let (mut client, repo) = stand.workspace().await;
    let target = target(&mut client, &repo, level).await;
    configure(&mut client, true, 0).await;

    let params = target.save_draft(target.overgrown(), Uuid::new_v4());
    let (saved, envelope) = command(&mut client, level.save_route(), params).await;
    let row = stand.newest_row().await;
    let answer: Option<String> = sqlx::query_scalar(
        "SELECT pg_catalog.convert_from(response_body, 'UTF8') FROM advice_journal \
         WHERE tenant_id=$1 ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(stand.tenant)
    .fetch_one(&stand.pool)
    .await
    .unwrap();
    assert!(!saved.to_string().contains(&key) && !row.to_string().contains(&key));
    assert!(answer.as_ref().is_none_or(|body| !body.contains(&key)));

    println!("level={} reply_envelope_bytes={envelope}", level.name());
    println!("advice={}", saved["advice"]);
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

    assert_eq!(saved["advice"]["status"], "advised", "{}", saved["advice"]);
    assert_eq!(row["advice"], saved["advice"]);
    assert_eq!(stand.count("advice_journal").await, 1);
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "calls the real Jev service; needs TECT_TEST_LIVE_JEV_KEY"]
async fn live_scope_draft_gets_advice_from_the_real_jev() {
    the_real_jev_answers_one_draft(Level::Scopes).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "calls the real Jev service; needs TECT_TEST_LIVE_JEV_KEY"]
async fn live_slice_draft_gets_advice_from_the_real_jev() {
    the_real_jev_answers_one_draft(Level::Slices).await;
}
