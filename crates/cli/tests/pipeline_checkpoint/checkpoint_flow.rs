use super::*;

pub(super) struct ResearchResolution {
    pub(super) completed: ResolvedPipeline,
    pub(super) accepted: ResolvedPipeline,
    pub(super) replay_resolve: Value,
}

pub(super) async fn complete_research_and_accept(
    client: &mut Mcp,
    mut research: ResolvedPipeline,
    checkpoint: &Value,
) -> ResearchResolution {
    let incomplete = json!({"request_id":Uuid::new_v4(),
        "producer_run_id":checkpoint["producer_run_id"],
        "producer_run_revision":checkpoint["producer_run_revision"],
        "checkpoint":checkpoint["checkpoint"],"action":"accept",
        "reason":"Attempt to advance before Research reaches a terminal result.",
        "terminal":{"result_id":Uuid::new_v4(),"output_id":Uuid::new_v4(),
            "output_digest":"nonterminal-research-output"}});
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.checkpoint.resolve",
            incomplete,
        )
        .await["error"]["code"],
        "forbidden"
    );
    while research.run()["current_phase_ordinal"].as_u64().unwrap() < 9 {
        research = advance(client, research).await;
    }
    let terminal = json!({"summary":"The exact constraint and limitation answer the checkpoint.",
        "evidence":[{"kind":"integration_test","reference":"pipeline_checkpoint.rs",
            "observation":"The terminal Research result is bound to the checkpoint."}],
        "scope_impact":"B05 can now reassess the exact answer criteria.",
        "remaining_work":"The producer still owns the decision."});
    let research_raw = route(
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
    .await;
    research = resolve_pipeline(client, research_raw).await.unwrap();
    while research.run()["current_phase_ordinal"].as_u64().unwrap() < 12 {
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
    let completed = resolve_pipeline(client, completed).await.unwrap();
    let result = &completed.details_data()["result"];
    let result_id = mutation_result_id(&completed);
    assert!(result_id.is_string());
    assert_eq!(result_id, &result["id"]);
    let output = completed.details_data()["outputs"]
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
    let accepted = resolve_pipeline(client, accepted).await.unwrap();
    assert_eq!(resolved_checkpoint(&accepted)["status"], "accepted");
    assert_eq!(accepted.run()["current_phase_id"], "B05");
    assert_eq!(accepted.run()["status"], "active");
    assert_eq!(
        route(
            client,
            "command",
            "slice.pipeline.checkpoint.resolve",
            resolve.clone()
        )
        .await,
        accepted.raw_payload
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

pub(super) fn resolved_checkpoint(resolved: &ResolvedPipeline) -> &Value {
    let reference = resolved
        .raw_payload
        .get("checkpoint_reference")
        .and_then(Value::as_object)
        .expect("actual checkpoint reference object");
    let id = reference
        .get("checkpoint")
        .expect("actual checkpoint identity key");
    assert!(id.is_string(), "checkpoint identity must be present");
    let matches: Vec<_> = resolved.details_data()["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|value| &value["checkpoint"] == id)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "exactly one pinned checkpoint matches the receipt"
    );
    let checkpoint = matches[0];
    assert_eq!(&checkpoint["checkpoint"], id);
    assert_eq!(
        reference
            .get("status")
            .expect("actual checkpoint status key"),
        &checkpoint["status"]
    );
    checkpoint
}
