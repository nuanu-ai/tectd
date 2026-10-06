use super::recovery_support::pipeline_reads::{ResolvedPipeline, resolve_pipeline};
use super::*;

pub(super) async fn refresh_pipeline_knowledge(
    client: &mut Mcp,
    context: &ResolvedPipeline,
    expected_state: &str,
) -> ResolvedPipeline {
    let observed_raw = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"]}),
    )
    .await;
    let observed = resolve_pipeline(client, observed_raw)
        .await
        .expect("resolve actual knowledge observation Current pipeline");
    assert_eq!(observed.run()["id"], context.run()["id"]);
    assert_eq!(observed.run()["revision"], context.run()["revision"]);
    assert_eq!(
        observed.run()["current_phase_id"],
        context.run()["current_phase_id"]
    );
    if observed.details_data()["knowledge_resource_status"]["state"] == "inactive" {
        assert_ne!(std::env::var("TECT_TEST_DK2").as_deref(), Ok("1"));
        assert!(observed.details_data()["knowledge_resources"].is_null());
        assert!(observed.details_data()["knowledge"].is_null());
        assert!(find_action(&observed.raw_payload, "pipeline.knowledge_refresh").is_none());
        return observed;
    }
    assert_eq!(
        observed.details_data()["knowledge_resource_status"]["state"],
        expected_state
    );
    if expected_state == "current" {
        assert_eq!(
            observed.details_data()["knowledge_resources"]["run_revision"],
            observed.run()["revision"]
        );
        assert!(find_action(&observed.raw_payload, "pipeline.knowledge_refresh").is_none());
        return observed;
    }
    assert_eq!(
        observed.run()["revision"].as_i64().unwrap(),
        observed.details_data()["knowledge_resources"]["run_revision"]
            .as_i64()
            .unwrap()
            + 1
    );
    let action = find_action(&observed.raw_payload, "pipeline.knowledge_refresh")
        .expect("stale pipeline knowledge must expose its exact refresh action");
    route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(action).clone(),
    )
    .await;
    let current_raw = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"]}),
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
