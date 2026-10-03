use super::*;

pub(super) async fn assert_slice_projection(
    client: &mut Mcp,
    scope_id: &Value,
    slice_id: &Value,
    run_id: &Value,
    status: &str,
) {
    let slice = route(
        client,
        "query",
        "slice.context",
        json!({"slice_id":slice_id}),
    )
    .await;
    assert_eq!(slice["pipeline_run_id"], *run_id);
    assert_eq!(slice["pipeline_status"], status);
    if !run_id.is_null() {
        let action = slice["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|action| action_name(action) == Some("slice.pipeline.context"))
            .unwrap();
        assert_eq!(action_params(action)["run_id"], *run_id);
    }
    let state = client.call("get_state", json!({})).await;
    let scope = state["native_planning"]
        .as_array()
        .unwrap()
        .iter()
        .find(|scope| scope["scope_id"] == *scope_id)
        .unwrap();
    let matching: Vec<_> = scope["pipeline_runs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|run| run["slice_id"] == *slice_id)
        .collect();
    if run_id.is_null() {
        assert!(matching.is_empty());
    } else {
        assert_eq!(
            matching.len(),
            1,
            "one Slice run projection per stored Slice"
        );
        assert_eq!(matching[0]["run_id"], *run_id);
        assert_eq!(matching[0]["status"], status);
    }
}

pub(super) async fn assert_persisted_status_boundaries(
    pool: &PgPool,
    client: &mut Mcp,
    scope_id: &Value,
    slice_id: &Value,
    successor_run_id: &Value,
) {
    // These persisted-state boundaries exercise the read model, not lifecycle
    // transitions. Only this isolated fixture's successor status is changed;
    // the complete original row must be restored afterwards.
    let successor_id = Uuid::parse_str(successor_run_id.as_str().unwrap()).unwrap();
    let before: Value =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM slice_pipeline_runs r WHERE id=$1")
            .bind(successor_id)
            .fetch_one(pool)
            .await
            .unwrap();
    for status in [
        "waiting_input",
        "blocked",
        "completed",
        "escalated",
        "superseded",
    ] {
        sqlx::query("UPDATE slice_pipeline_runs SET status=$2 WHERE id=$1")
            .bind(successor_id)
            .bind(status)
            .execute(pool)
            .await
            .unwrap();
        let expected = if status == "superseded" {
            Value::Null
        } else {
            successor_run_id.clone()
        };
        assert_slice_projection(
            client,
            scope_id,
            slice_id,
            &expected,
            if status == "superseded" {
                "not_started"
            } else {
                status
            },
        )
        .await;
    }
    sqlx::query("UPDATE slice_pipeline_runs SET status=$2 WHERE id=$1")
        .bind(successor_id)
        .bind(before["status"].as_str().unwrap())
        .execute(pool)
        .await
        .unwrap();
    let after: Value =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM slice_pipeline_runs r WHERE id=$1")
            .bind(successor_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        before, after,
        "status fixture restores the complete successor row"
    );
}
