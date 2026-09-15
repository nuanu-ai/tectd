use super::*;

pub(super) async fn rework_and_complete_producer(
    client: &mut Mcp,
    pool: &PgPool,
    accepted: &Value,
    replay_resolve: &Value,
) -> Value {
    let producer = refresh_or_capture_knowledge(client, &accepted["context"]).await;
    let second_checkpoint = create_checkpoint(client, &producer).await;
    assert!(second_checkpoint["basis"]["consumed_knowledge"].is_object());
    let generation: i64 = sqlx::query_scalar(
        "UPDATE workspace_knowledge_state state SET generation=state.generation+1 FROM slice_pipeline_runs run WHERE run.id=$1 AND state.tenant_id=run.tenant_id AND state.workspace_id=run.workspace_id RETURNING state.generation",
    )
    .bind(second_checkpoint["producer_run_id"].as_str().unwrap().parse::<Uuid>().unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(generation > 0);
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.checkpoint.resolve",
            json!({"request_id":Uuid::new_v4(),
                "producer_run_id":second_checkpoint["producer_run_id"],
                "producer_run_revision":second_checkpoint["producer_run_revision"],
                "checkpoint":second_checkpoint["checkpoint"],"action":"accept",
                "reason":"A stale basis cannot be accepted.",
                "terminal":replay_resolve["terminal"]}),
        )
        .await["error"]["code"],
        "stale_context"
    );
    let cancelled = route(
        client,
        "command",
        "slice.pipeline.checkpoint.resolve",
        json!({"request_id":Uuid::new_v4(),
            "producer_run_id":second_checkpoint["producer_run_id"],
            "producer_run_revision":second_checkpoint["producer_run_revision"],
            "checkpoint":second_checkpoint["checkpoint"],"action":"cancel",
            "reason":"The exact basis became stale and the producer must refresh."}),
    )
    .await;
    assert_eq!(cancelled["checkpoint"]["status"], "cancelled");
    assert_eq!(cancelled["context"]["run"]["current_phase_id"], "B05");
    let producer = refresh_or_capture_knowledge(client, &cancelled["context"]).await;
    let second_checkpoint = create_checkpoint(client, &producer).await;
    let waiting = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":producer["run"]["id"]}),
    )
    .await;
    let reworked = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &waiting,
            "rework",
            "completed",
            "continue",
            Some("B04"),
            None,
        ),
    )
    .await["context"]
        .clone();
    assert_eq!(reworked["run"]["current_phase_id"], "B04");
    let superseded = reworked["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["checkpoint"] == second_checkpoint["checkpoint"])
        .unwrap();
    assert_eq!(superseded["status"], "superseded");

    let mut producer = reworked;
    while producer["run"]["current_phase_ordinal"].as_u64().unwrap() < 8 {
        producer = advance(client, producer).await;
    }
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.phase.complete",
            completion(
                &producer,
                "recommended",
                "completed",
                "continue",
                None,
                None,
            ),
        )
        .await["error"]["code"],
        "forbidden"
    );
    let pending = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &producer,
            "pending_decision",
            "waiting_input",
            "continue",
            None,
            None,
        ),
    )
    .await["context"]
        .clone();
    assert_eq!(pending["run"]["status"], "waiting_input");
    assert_eq!(pending["run"]["current_phase_id"], "B08");
    let input = route(
        client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":pending["run"]["id"],
            "run_revision":pending["run"]["revision"],"phase_id":"B08",
            "input":"The decision owner now selects the supported alternative."}),
    )
    .await;
    producer = refresh_or_capture_knowledge(client, &input["context"]).await;
    producer = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&producer, "selected", "completed", "continue", None, None),
    )
    .await["context"]
        .clone();
    let mut mismatched = completion(&producer, "ready", "completed", "continue", None, None);
    mismatched["output"]["fields"]["disposition"] = json!("recommended");
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.phase.complete",
            mismatched,
        )
        .await["error"]["code"],
        "stale_context"
    );
    producer = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&producer, "ready", "completed", "continue", None, None),
    )
    .await["context"]
        .clone();
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.phase.complete",
            completion(
                &producer,
                "recommended",
                "completed",
                "complete",
                None,
                Some(
                    json!({"summary":"A recommendation cannot complete a decision inquiry.",
                    "evidence":[{"kind":"integration_test","reference":"pipeline_checkpoint.rs",
                        "observation":"The requested outcome is an actual decision."}],
                    "scope_impact":"No decision is recorded.",
                    "remaining_work":"Record the selected disposition."})
                ),
            ),
        )
        .await["error"]["code"],
        "forbidden"
    );
    let decided = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &producer,
            "selected",
            "completed",
            "complete",
            None,
            Some(
                json!({"summary":"The selected decision matches B08 and B09.",
                "evidence":[{"kind":"integration_test","reference":"pipeline_checkpoint.rs",
                    "observation":"B08 and B09 carry the same selected disposition."}],
                "scope_impact":"The decision inquiry is complete.",
                "remaining_work":"Execution remains separate."}),
            ),
        ),
    )
    .await;
    assert_eq!(decided["context"]["run"]["status"], "completed");

    decided
}
