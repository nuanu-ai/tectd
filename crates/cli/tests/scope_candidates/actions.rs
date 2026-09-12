use crate::recovery_support::{action_name, action_params};
use serde_json::Value;
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
