//! What the `pipeline` tests do beyond the shared fixtures: bring an opened Scope to
//! the point where a Slice draft can be saved, save drafts under known request ids,
//! and read the journal rows of one request.
use crate::drafts::{Level, target};
use crate::recovery_support::Mcp;
use crate::support::{Stand, command};
use crate::walk::SliceSet;
use serde_json::{Value, json};
use std::path::Path;
use uuid::Uuid;

pub const SAVE: &str = "slice.candidates.save";
pub const DEBUG: &str = "slice.debug-root-cause";
pub const LIGHTWEIGHT: &str = "slice.lightweight-tdd-development";
pub const RESEARCH: &str = "slice.research";

/// The Slice planning of an opened Scope that accepts a first draft, and the Program
/// behind it. Jev plays no part up to here.
pub async fn planning(client: &mut Mcp, repo: &Path) -> (SliceSet, Value) {
    let target = target(client, repo, Level::Slices).await;
    (
        SliceSet {
            context: target.context,
        },
        target.program_id,
    )
}

/// Enable Jev for the client's workspace with this daily call limit.
pub async fn enable(client: &mut Mcp, daily_call_limit: i64, expected_revision: i64) {
    let params = json!({"enabled":true,"daily_call_limit":daily_call_limit,
        "expected_revision":expected_revision});
    let (status, _) = command(client, "workspace.advisory.configure", params).await;
    assert_eq!(status["enabled"], true, "{status}");
    assert_eq!(status["daily_call_limit"], daily_call_limit, "{status}");
}

/// Save a draft at this head: the reply, the head it left, and the request id.
pub async fn save(client: &mut Mcp, head: &SliceSet, draft: Value) -> (Value, SliceSet, Uuid) {
    let params = head.save(draft);
    let request_id = Uuid::parse_str(params["request_id"].as_str().unwrap()).unwrap();
    let (reply, envelope) = command(client, SAVE, params).await;
    assert!(envelope <= crate::support::BUDGET, "{envelope}");
    assert_eq!(
        reply["candidate_set"]["status"], "review_required",
        "{reply}"
    );
    let next = SliceSet::after(client, reply.clone()).await;
    (reply, next, request_id)
}

/// A new work node under a local label, with the pipeline the agent chose and a
/// reason no other text of the fixtures holds.
pub fn work(number: usize, pipeline: &str, reason: &str) -> Value {
    json!({"kind":"work","identity":{"local":format!("work-{number}")},
        "title":format!("Work {number}"),"outcome":"The cause is demonstrated",
        "includes":["item 1"],"excludes":["deployment"],"dependencies":[],
        "proof":["Causal observation"],"pipeline":pipeline,"pipeline_reason":reason,
        "source_result_ids":[]})
}

/// A new decision node under a local label.
pub fn decision(label: &str, question: &str) -> Value {
    json!({"kind":"decision","identity":{"local":label},"title":"Choose correction path",
        "question":question,"resolution_criteria":["Diagnosis identifies the boundary"],
        "dependencies":[],"source_result_ids":[]})
}

/// A stored node stated again for a later save, unchanged: work or decision.
pub fn kept(node: &Value) -> Value {
    let identity = json!({"candidate_id":node["id"],"revision":node["revision"]});
    if node["kind"] == "decision" {
        return json!({"kind":"decision","identity":identity,"title":node["title"],
            "question":node["question"],"resolution_criteria":node["resolution_criteria"],
            "dependencies":[],"source_result_ids":[]});
    }
    json!({"kind":"work","identity":identity,"title":node["title"],
        "outcome":node["outcome"],"includes":node["includes"],"excludes":node["excludes"],
        "dependencies":[],"proof":node["proof"],"pipeline":node["pipeline"],
        "pipeline_reason":node["pipeline_reason"],"source_result_ids":[]})
}

pub fn draft(nodes: Vec<Value>) -> Value {
    json!({"coverage_summary":"The whole Scope","nodes":nodes,"supersessions":[]})
}

/// The stored nodes of the head's draft, as its `details` read shows them.
pub fn stored(head: &SliceSet) -> Vec<Value> {
    head.context["draft"]["nodes"].as_array().unwrap().clone()
}

/// The journal rows one request wrote, oldest first, as the operator sees them, with
/// the request body as JSON.
pub async fn rows(stand: &Stand, request_id: Uuid) -> Vec<Value> {
    sqlx::query_scalar(
        "SELECT pg_catalog.to_jsonb(j) - 'request_body' - 'response_body' \
             || pg_catalog.jsonb_build_object('request', \
                 pg_catalog.convert_from(request_body, 'UTF8')::jsonb) \
         FROM advice_journal j WHERE tenant_id=$1 AND request_id=$2 \
         ORDER BY created_at, id",
    )
    .bind(stand.tenant)
    .bind(request_id)
    .fetch_all(&stand.pool)
    .await
    .unwrap()
}

/// The advice points of those rows, in the order they were written.
pub fn points(rows: &[Value]) -> Vec<&str> {
    rows.iter()
        .map(|row| row["point"].as_str().unwrap())
        .collect()
}

/// The names of the questions of one request to Jev, in the order it holds them.
pub fn asked(request: &Value) -> Vec<&String> {
    request["questions"].as_object().unwrap().keys().collect()
}
