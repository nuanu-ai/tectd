use super::knowledge_lifecycle_support::begin_create_request;
use super::recovery_support::Mcp;
use super::support::{open_slice, ready_source_candidate, review, route, save};
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

fn draft() -> Value {
    json!({"coverage_summary":"One bounded durable publication is owned by Knowledge Change.",
        "nodes":[{"kind":"work","identity":{"local":"promotion"},
        "title":"Publish the exact fixture declaration",
        "outcome":"The reviewed declaration is current durable knowledge",
        "includes":["qualified publication","managed result"],
        "excludes":["new research","operational execution"],"dependencies":[],
        "proof":["Knowledge Change publisher receipt"],
        "pipeline":"slice.promote-to-durable-knowledge",
        "pipeline_reason":"The bounded outcome is durable creation from available evidence.",
        "source_result_ids":[]}],"supersessions":[]})
}

pub struct PromotionSeed {
    pub slice: Value,
    pub document: Value,
    pub begun: Value,
    pub begin_request: Value,
}

pub async fn open(client: &mut Mcp, repo: &Path, marker: &str) -> PromotionSeed {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope = route(
        client,
        "command",
        "scope.open",
        json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
        "candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = save(client, &scope["created"]["planning"], draft()).await;
    let reviewed = review(client, &saved).await;
    let slice = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await["created"]
        .clone();
    assert_eq!(slice["pipeline"], "slice.promote-to-durable-knowledge");
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let mut document = fixture["document"].clone();
    document["sources"][0]["snapshot"]["text"] = json!(marker);
    document["sources"][0]["snapshot"]["uri"] = json!(format!("urn:{marker}"));
    let begin_request = begin_create_request(
        &document,
        json!({"kind":"promotion_slice","scope_id":slice["scope_id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"]}),
        Uuid::new_v4(),
    );
    let begun = route(
        client,
        "command",
        "knowledge.change_begin",
        begin_request.clone(),
    )
    .await;
    PromotionSeed {
        slice,
        document,
        begun,
        begin_request,
    }
}
