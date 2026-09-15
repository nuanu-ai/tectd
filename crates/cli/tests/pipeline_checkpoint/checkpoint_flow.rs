use super::*;

pub(super) struct ResearchResolution {
    pub(super) completed: Value,
    pub(super) accepted: Value,
    pub(super) replay_resolve: Value,
}

pub(super) async fn complete_research_and_accept(
    client: &mut Mcp,
    mut research: Value,
    checkpoint: &Value,
) -> ResearchResolution {
    while research["run"]["current_phase_ordinal"].as_u64().unwrap() < 9 {
        research = advance(client, research).await;
    }
    let terminal = json!({"summary":"The exact constraint and limitation answer the checkpoint.",
        "evidence":[{"kind":"integration_test","reference":"pipeline_checkpoint.rs",
            "observation":"The terminal Research result is bound to the checkpoint."}],
        "scope_impact":"B05 can now reassess the exact answer criteria.",
        "remaining_work":"The producer still owns the decision."});
    research = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &research,
            "bounded_inconclusive",
            "completed",
            "continue",
            None,
            None,
        ),
    )
    .await["context"]
        .clone();
    while research["run"]["current_phase_ordinal"].as_u64().unwrap() < 12 {
        research = advance(client, research).await;
    }
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.phase.complete",
            completion(
                &research,
                "answered",
                "completed",
                "complete",
                None,
                Some(terminal.clone()),
            ),
        )
        .await["error"]["code"],
        "stale_context"
    );
    let completed = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &research,
            "inconclusive",
            "completed",
            "complete",
            None,
            Some(terminal),
        ),
    )
    .await;
    let result = &completed["result"];
    let output = completed["context"]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["phase_ordinal"] == 12)
        .unwrap();
    let mut resolve = json!({"request_id":Uuid::new_v4(),
        "producer_run_id":checkpoint["producer_run_id"],
        "producer_run_revision":checkpoint["producer_run_revision"],
        "checkpoint":checkpoint["checkpoint"],"action":"accept",
        "reason":"The returned answer meets the recorded criteria.",
        "terminal":{"result_id":result["id"],"output_id":output["id"],"output_digest":output["digest"]}});
    let replay_resolve = resolve.clone();
    let wrong = {
        let mut value = resolve.clone();
        value["request_id"] = json!(Uuid::new_v4());
        value["terminal"]["result_id"] = json!(Uuid::new_v4());
        value
    };
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.checkpoint.resolve",
            wrong
        )
        .await["error"]["code"],
        "forbidden"
    );
    let accepted = route(
        client,
        "command",
        "slice.pipeline.checkpoint.resolve",
        resolve.clone(),
    )
    .await;
    assert_eq!(accepted["checkpoint"]["status"], "accepted");
    assert_eq!(accepted["context"]["run"]["current_phase_id"], "B05");
    assert_eq!(accepted["context"]["run"]["status"], "active");
    assert_eq!(
        route(
            client,
            "command",
            "slice.pipeline.checkpoint.resolve",
            resolve.clone()
        )
        .await,
        accepted
    );
    resolve["reason"] = json!("Changed replay payload.");
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.checkpoint.resolve",
            resolve
        )
        .await["error"]["code"],
        "input_conflict"
    );

    ResearchResolution {
        completed,
        accepted,
        replay_resolve,
    }
}
