use super::recovery_support::pipeline_reads::{ResolvedPipeline, resolve_pipeline};
use super::*;

pub(super) async fn escalate_and_verify(
    client: &mut Mcp,
    context: &ResolvedPipeline,
) -> ResolvedPipeline {
    let before_escalation = context;
    let escalation_request = json!({"request_id":Uuid::new_v4(),"run_id":context.run()["id"],
        "run_revision":context.run()["revision"],"phase_id":context.run()["current_phase_id"],
        "reason":"The remaining context now warrants phasewise delivery."});
    let escalated = route(
        client,
        "command",
        "slice.pipeline.delivery.escalate",
        escalation_request.clone(),
    )
    .await;
    assert_eq!(
        route(
            client,
            "command",
            "slice.pipeline.delivery.escalate",
            escalation_request.clone()
        )
        .await,
        escalated
    );
    let mut conflicting = escalation_request.clone();
    conflicting["reason"] = json!("Changed replay payload");
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.delivery.escalate",
            conflicting
        )
        .await["error"]["code"],
        "input_conflict"
    );
    let mut stale = escalation_request;
    stale["request_id"] = json!(Uuid::new_v4());
    assert_eq!(
        route_error(client, "command", "slice.pipeline.delivery.escalate", stale).await["error"]["code"],
        "stale_revision"
    );
    let context = resolve_pipeline(client, escalated)
        .await
        .expect("resolve actual delivery escalation pipeline");
    assert_eq!(context.run()["id"], before_escalation.run()["id"]);
    assert_eq!(
        context.run()["revision"].as_i64().unwrap(),
        before_escalation.run()["revision"].as_i64().unwrap() + 1
    );
    assert_eq!(context.run()["delivery_mode"], "phasewise");
    assert_eq!(
        context.run()["current_phase_id"],
        before_escalation.run()["current_phase_id"]
    );
    assert_eq!(
        context.details_data()["outputs"],
        before_escalation.details_data()["outputs"]
    );
    assert_eq!(
        context.details_data()["bindings"],
        before_escalation.details_data()["bindings"]
    );
    context
}
