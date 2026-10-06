use crate::recovery_support::Mcp;
use crate::recovery_support::candidate_reads::{read_query_json, read_ready_json};
use crate::recovery_support::candidate_reviews::source_chunk_next_action;
use crate::recovery_support::{action_name, action_params};
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

pub(super) fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

pub(super) fn candidate_action(
    action: &Value,
    route: &str,
    kind: Option<&str>,
    set: Uuid,
    revision: i64,
) -> Value {
    assert_eq!(action_name(action), Some(route));
    let params = action_params(action);
    assert_eq!(params["candidate_set_id"], set.to_string());
    assert_eq!(params["revision"], revision);
    if let Some(kind) = kind {
        assert_eq!(params["kind"], kind);
    }
    id(&params["request_id"]);
    params.clone()
}

pub(super) async fn rows(pool: &PgPool, set: Uuid) -> (i64, i64, i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT
          (SELECT count(*) FROM scope_candidate_sets WHERE id=$1),
          (SELECT count(*) FROM scope_candidate_inputs WHERE candidate_set_id=$1),
          (SELECT count(*) FROM scope_candidate_snapshots WHERE candidate_set_id=$1),
          (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1),
          (SELECT count(*) FROM scope_candidate_reviews WHERE candidate_set_id=$1),
          (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1)",
    )
    .bind(set)
    .fetch_one(pool)
    .await
    .unwrap()
}

pub(super) async fn create_program(client: &mut Mcp, input: &str, name: &str) -> Uuid {
    let created = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":input}),
        )
        .await;
    let program = id(&created["program"]["id"]);
    let saved = client
        .call(
            "save_program",
            json!({
                "program_id":program,"revision":1,"input_cursor":1,"name":name,
                "intent":"Inspect \"email\" notification preferences 🧭",
                "basis":"The captured request\nand accepted work",
                "boundaries":"Email only; exclude SMS, push, and analytics",
                "constraints":"Read only; preserve accepted work and \\slashes",
                "success":"Users can inspect email preferences and tests pass",
                "complete":true
            }),
        )
        .await;
    assert_eq!(saved["program"]["status"], "open");
    program
}
pub(super) fn draft(
    boundary: &str,
    goal: Value,
    evidence: Vec<Value>,
    candidate: Value,
    protected_changes: Vec<Value>,
) -> Value {
    json!({
        "boundary":boundary,
        "goals":[goal],"evidence":evidence,"candidates":[candidate],"blockers":[],
        "protected_changes":protected_changes
    })
}

pub(super) async fn text(client: &mut Mcp, set: Uuid, source: Uuid) -> String {
    let mut cursor = 0_u64;
    let mut continuation = None::<Value>;
    let mut result = String::new();
    loop {
        let page = if let Some(action) = continuation.take() {
            read_ready_json(client, &action).await
        } else {
            read_query_json(client, &serde_json::json!({"route":"scope.candidates.context","params":json!({"candidate_set_id":set,"view":"fragment","source_ref_id":source,"cursor":cursor})})).await
        };
        let fragment = &page.value["fragment"];
        assert_eq!(fragment["cursor"], cursor);
        let part = fragment["text"].as_str().unwrap();
        assert!(!part.is_empty() || fragment["next_cursor"].is_null());
        result.push_str(part);
        let Some(next) = fragment["next_cursor"].as_u64() else {
            break;
        };
        assert!(next > cursor);
        continuation = Some(source_chunk_next_action(&page, next));
        cursor = next;
    }
    result
}

pub(super) fn advertised_refresh(
    payload: &Value,
    set: Uuid,
    revision: i64,
    program_revision: i64,
) -> Value {
    assert_eq!(payload["recommended_action"], 0);
    assert_eq!(payload["actions"].as_array().unwrap().len(), 2);
    let overview = &payload["actions"][0];
    assert_eq!(overview["kind"], "ready_call");
    assert_eq!(overview["tool"], "query");
    assert_eq!(action_name(overview), Some("scope.candidates.context"));
    assert_eq!(action_params(overview)["candidate_set_id"], set.to_string());
    assert_eq!(action_params(overview)["view"], "overview");
    let matching = payload["actions"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|action| {
            action["kind"] == "ready_call"
                && action["tool"] == "command"
                && action_name(action) == Some("scope.candidates.refresh")
                && action_params(action)["candidate_set_id"] == set.to_string()
                && action_params(action)["revision"] == revision
                && action_params(action)["program_revision"] == program_revision
        })
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "actual compact Refresh descriptor");
    let params = action_params(matching[0]);
    id(&params["request_id"]);
    params.clone()
}
