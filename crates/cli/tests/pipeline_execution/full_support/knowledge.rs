use super::*;

pub(crate) async fn refresh_knowledge(
    client: &mut Mcp,
    context: &ResolvedPipeline,
) -> ResolvedPipeline {
    let stale_raw = client
        .call(
            "query",
            json!({"route":"slice.pipeline.context","params":{"run_id":context.run()["id"]}}),
        )
        .await;
    let stale = resolve_pipeline(client, stale_raw)
        .await
        .expect("resolve actual pre-refresh Current pipeline");
    assert_eq!(stale.run()["id"], context.run()["id"]);
    assert_eq!(stale.run()["revision"], context.run()["revision"]);
    assert_eq!(
        stale.run()["current_phase_id"],
        context.run()["current_phase_id"]
    );
    if stale.details_data()["knowledge_resource_status"]["state"] == "inactive" {
        assert!(stale.details_data()["knowledge_resources"].is_null());
        assert!(stale.details_data()["knowledge"].is_null());
        assert!(find_action(&stale.raw_payload, "pipeline.knowledge_refresh").is_none());
        return stale;
    }
    if stale.details_data()["knowledge_resource_status"]["state"] == "current" {
        assert_eq!(
            stale.details_data()["knowledge_resources"]["run_revision"],
            stale.run()["revision"]
        );
        assert!(find_action(&stale.raw_payload, "pipeline.knowledge_refresh").is_none());
        return stale;
    }
    let resource_state = stale.details_data()["knowledge_resource_status"]["state"]
        .as_str()
        .unwrap();
    assert!(
        matches!(resource_state, "stale" | "needs_context"),
        "{}",
        stale.raw_payload
    );
    if resource_state == "stale" {
        assert_eq!(
            stale.run()["revision"].as_i64().unwrap(),
            stale.details_data()["knowledge_resources"]["run_revision"]
                .as_i64()
                .unwrap()
                + 1
        );
    }
    let action = find_action(&stale.raw_payload, "pipeline.knowledge_refresh")
        .expect("stale pipeline knowledge must expose its exact refresh action");
    client
        .call(
            "command",
            json!({"route":"pipeline.knowledge_refresh","params":action_params(action)}),
        )
        .await;
    let current_raw = client
        .call(
            "query",
            json!({"route":"slice.pipeline.context","params":{"run_id":context.run()["id"]}}),
        )
        .await;
    let current = resolve_pipeline(client, current_raw)
        .await
        .expect("resolve actual post-refresh Current pipeline");
    assert_eq!(
        current.details_data()["knowledge_resource_status"]["state"],
        "current"
    );
    current
}
