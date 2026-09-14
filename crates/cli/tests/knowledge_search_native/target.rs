use crate::{recovery_support::Mcp, support};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::Path;
use uuid::Uuid;

fn draft(label: &str) -> Value {
    json!({"coverage_summary":format!("Open isolated DK3 target {label}."),"nodes":[{
        "kind":"work","identity":{"local":"search"},"title":format!("Search target {label}"),
        "outcome":"One bounded native search target exists","includes":["DK3 search acceptance"],
        "excludes":["deployment"],"dependencies":[],"proof":["Real MCP receipt"],
        "pipeline":"slice.custom-procedure-capture","pipeline_reason":"Exercise bound search",
        "source_result_ids":[]}],"supersessions":[]})
}

pub async fn open(client: &mut Mcp, repo: &Path, pool: &PgPool, label: &str) -> Value {
    let (source, candidate) = support::ready_source_candidate(client, repo).await;
    let scope = support::route(
        client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
            "candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = support::save(client, &scope["created"]["planning"], draft(label)).await;
    let reviewed = support::review(client, &saved).await;
    let slice = support::route(
        client,
        "command",
        "slice.open",
        support::open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let scope_id = Uuid::parse_str(reviewed["scope"]["id"].as_str().unwrap()).unwrap();
    let program_id: Uuid = sqlx::query_scalar(
        "SELECT cs.program_id FROM native_scopes ns JOIN scope_candidate_sets cs \
         ON cs.tenant_id=ns.tenant_id AND cs.workspace_id=ns.workspace_id \
         AND cs.id=ns.source_candidate_set_id WHERE ns.id=$1",
    )
    .bind(scope_id)
    .fetch_one(pool)
    .await
    .unwrap();
    json!({"program_id":program_id,"scope_id":scope_id,
        "slice_id":Uuid::parse_str(slice["created"]["id"].as_str().unwrap()).unwrap()})
}
