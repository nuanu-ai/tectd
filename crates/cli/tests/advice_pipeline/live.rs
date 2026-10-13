//! One real call of the `pipeline` point to the Jev service, beside the real `sizing`
//! call of the same save. Ignored by default: the calls spend money and need the key
//! in `TECT_TEST_LIVE_JEV_KEY`. Run it alone and read what it prints:
//!
//! `cargo test -p tect-cli --test advice_pipeline live -- --ignored --nocapture`
use crate::pipeline::{LIGHTWEIGHT, draft, planning, points, rows, save, work};
use crate::support::{Stand, configure};
use serde_json::{Value, json};

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const FULL: &str = "slice.full-design-to-execution";

/// A new work node with its own words, so that Jev has something to tell apart.
fn described(number: usize, pipeline: &str, title: &str, outcome: &str, includes: Value) -> Value {
    let mut node = work(number, pipeline, "chosen by the planning agent");
    node["title"] = json!(title);
    node["outcome"] = json!(outcome);
    node["includes"] = includes;
    if pipeline == FULL {
        node["why_lightweight_insufficient"] = json!("stated by the planning agent");
        node["why_further_vertical_split_not_viable"] = json!("stated by the planning agent");
    }
    node
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
#[ignore = "calls the real Jev service; needs TECT_TEST_LIVE_JEV_KEY"]
async fn live_slice_draft_gets_pipeline_advice_from_the_real_jev() {
    let key = std::env::var("TECT_TEST_LIVE_JEV_KEY").expect("TECT_TEST_LIVE_JEV_KEY required");
    let stand = Stand::start_at(Some(ENDPOINT), &key, "30000").await;
    let (mut client, repo) = stand.workspace().await;
    let (head, _) = planning(&mut client, &repo).await;
    configure(&mut client, true, 0).await;

    // Four new work nodes. The draft chose Lightweight for work whose cause is
    // unknown, Lightweight for a bounded change, and Full for a comparison of options.
    let nodes = vec![
        described(
            1,
            LIGHTWEIGHT,
            "Find why invoice totals are wrong after a partial refund",
            "The cause of the wrong totals is demonstrated on a reproducible input; no fix is made",
            json!([
                "Reproduce the wrong total",
                "Trace the calculation to the faulty step"
            ]),
        ),
        described(
            2,
            LIGHTWEIGHT,
            "Add a currency code field to the invoice form",
            "An invoice can be saved with a three-letter currency code that is validated",
            json!(["The form field", "Validation of the code", "A test of both"]),
        ),
        described(
            3,
            FULL,
            "Compare three payment providers and recommend one",
            "A written comparison of fees, limits and integration effort with a recommendation",
            json!([
                "Collect the published terms",
                "Compare them",
                "Write the recommendation"
            ]),
        ),
        // The case of the stand probes of 2026-10-13: a document of the repository, for
        // which Jev chose the knowledge-base and the operational pipelines.
        described(
            4,
            LIGHTWEIGHT,
            "Write the initial proof matrix",
            "docs/proof-matrix.md maps every required service and UI to its invariant, local command and proving artifact",
            json!([
                "One row per required service",
                "The local command that proves each row",
                "A check that the file lists every service"
            ]),
        ),
    ];
    let (saved, _, request_id) = save(&mut client, &head, draft(nodes)).await;
    let written = rows(&stand, request_id).await;
    assert_eq!(points(&written), ["sizing", "pipeline"]);
    let row = &written[1];
    let answer: Option<String> = sqlx::query_scalar(
        "SELECT pg_catalog.convert_from(response_body, 'UTF8') FROM advice_journal \
         WHERE tenant_id=$1 AND point='pipeline' ORDER BY created_at DESC, id DESC LIMIT 1",
    )
    .bind(stand.tenant)
    .fetch_one(&stand.pool)
    .await
    .unwrap();
    assert!(!saved.to_string().contains(&key) && !json!(written).to_string().contains(&key));
    assert!(answer.as_ref().is_none_or(|body| !body.contains(&key)));

    println!("sizing_advice={}", saved["advice"]);
    println!("pipeline_advice={}", saved["pipeline_advice"]);
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
        saved["pipeline_advice"]["status"], "advised",
        "{}",
        saved["pipeline_advice"]
    );
    assert_eq!(row["advice"], saved["pipeline_advice"]);
    assert_eq!(
        saved["pipeline_advice"]["candidates"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    client.finish().await;
}
