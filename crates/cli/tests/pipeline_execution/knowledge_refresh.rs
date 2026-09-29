use super::*;

pub(super) async fn refresh_pipeline_knowledge(
    client: &mut Mcp,
    context: &serde_json::Value,
) -> serde_json::Value {
    let stale = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["run"]["id"], context["run"]["id"]);
    assert_eq!(stale["run"]["revision"], context["run"]["revision"]);
    assert_eq!(
        stale["run"]["current_phase_id"],
        context["run"]["current_phase_id"]
    );
    if stale["knowledge_resource_status"]["state"] == "inactive" {
        assert!(stale["knowledge_resources"].is_null());
        assert!(stale["knowledge"].is_null());
        assert!(find_action(&stale, "pipeline.knowledge_refresh").is_none());
        return stale;
    }
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
    assert_eq!(
        stale["run"]["revision"].as_i64().unwrap(),
        stale["knowledge_resources"]["run_revision"]
            .as_i64()
            .unwrap()
            + 1
    );
    let action = find_action(&stale, "pipeline.knowledge_refresh")
        .expect("stale pipeline knowledge must expose its exact refresh action");
    route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(action).clone(),
    )
    .await;
    let current = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(current["knowledge_resource_status"]["state"], "current");
    current
}
