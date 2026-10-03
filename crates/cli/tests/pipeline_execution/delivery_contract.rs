use super::*;
use serde_json::Value;

pub(super) async fn escalate_and_verify(client: &mut Mcp, context: &Value) -> Value {
    let before_escalation = context.clone();
    let escalation_request = json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
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
    let context = escalated["context"].clone();
    assert_eq!(
        context["run"]["current_phase_id"],
        before_escalation["run"]["current_phase_id"]
    );
    assert_eq!(context["outputs"], before_escalation["outputs"]);
    assert_eq!(context["bindings"], before_escalation["bindings"]);
    context
}
