use super::*;

pub(super) async fn refresh_pipeline_knowledge(
    client: &mut Mcp,
    context: &serde_json::Value,
    expected_state: &str,
) -> serde_json::Value {
    let observed = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(observed["run"]["id"], context["run"]["id"]);
    assert_eq!(observed["run"]["revision"], context["run"]["revision"]);
    assert_eq!(
        observed["run"]["current_phase_id"],
        context["run"]["current_phase_id"]
    );
    if observed["knowledge_resource_status"]["state"] == "inactive" {
        assert_ne!(std::env::var("TECT_TEST_DK2").as_deref(), Ok("1"));
        assert!(observed["knowledge_resources"].is_null());
        assert!(observed["knowledge"].is_null());
        assert!(find_action(&observed, "pipeline.knowledge_refresh").is_none());
        return observed;
    }
    assert_eq!(
        observed["knowledge_resource_status"]["state"],
        expected_state
    );
    if expected_state == "current" {
        assert_eq!(
            observed["knowledge_resources"]["run_revision"],
            observed["run"]["revision"]
        );
        assert!(find_action(&observed, "pipeline.knowledge_refresh").is_none());
        return observed;
    }
    assert_eq!(
        observed["run"]["revision"].as_i64().unwrap(),
        observed["knowledge_resources"]["run_revision"]
            .as_i64()
            .unwrap()
            + 1
    );
    let action = find_action(&observed, "pipeline.knowledge_refresh")
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
