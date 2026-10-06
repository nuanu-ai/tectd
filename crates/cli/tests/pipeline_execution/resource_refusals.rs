use super::*;
use serde_json::Value;

// A mutation publication reference describes this response, independently of retained history.
pub(super) fn result_reference_id(raw_payload: &Value) -> &Value {
    raw_payload
        .get("result_reference")
        .and_then(Value::as_object)
        .expect("mutation must carry a result reference object")
        .get("result_id")
        .expect("mutation result reference must carry result_id")
}

pub(super) fn published_result(
    context: &recovery_support::pipeline_reads::ResolvedPipeline,
) -> &Value {
    let result_id = result_reference_id(&context.raw_payload);
    assert!(result_id.as_str().is_some_and(|id| !id.is_empty()));
    let result = &context.details_data()["result"];
    assert_eq!(
        result.get("id").expect("published result ID missing"),
        result_id
    );
    result
}

// A supplied receipt is refused before any phase completion can persist.
pub(super) async fn assert_backend_proof_refusal(
    client: &mut Mcp,
    before: &recovery_support::pipeline_reads::ResolvedPipeline,
    response: &Value,
) {
    assert_eq!(response["error"]["code"], "BACKEND_DERIVED_PROOF_REQUIRED");
    let refusal = &response["error"]["refusal"];
    for (field, expected) in [
        ("code", "BACKEND_DERIVED_PROOF_REQUIRED"),
        ("rule", "WP3-PROOF-01"),
        ("path", "arguments.params.consumed_outputs"),
        ("expected", "omitted; backend derives the proof"),
        ("actual", "agent-supplied value"),
        ("next_action", "omit_agent_supplied_proof"),
        ("required", "backend_derived_proof"),
    ] {
        assert_eq!(refusal[field], expected);
    }
    let raw = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":before.run()["id"]}),
    )
    .await;
    let after = resolve_pipeline(client, raw).await.unwrap();
    assert_eq!(after.run(), before.run());
    for collection in ["attempts", "outputs", "bindings", "inputs"] {
        assert_eq!(
            after.details_data()[collection],
            before.details_data()[collection]
        );
    }
}

// Deliberate public input, independent of the successful builder's omitted proof.
pub(super) fn supplied_review_proof(
    context: &recovery_support::pipeline_reads::ResolvedPipeline,
) -> Value {
    let bindings = context.details_data()["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|binding| binding["phase_id"] == "K3" && binding["stale"] == false)
        .collect::<Vec<_>>();
    assert_eq!(bindings.len(), 1);
    let binding = bindings[0];
    let outputs = context.details_data()["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|output| {
            output["phase_id"] == binding["phase_id"]
                && output["revision"] == binding["output_revision"]
                && output["stale"] == false
        })
        .collect::<Vec<_>>();
    assert_eq!(outputs.len(), 1);
    let output = outputs[0];
    assert_eq!(output["digest"], binding["output_digest"]);
    let mut receipt = tect_domain::PipelineConsumedOutput {
        phase_id: binding["phase_id"].as_str().unwrap().to_owned(),
        output_revision: binding["output_revision"].as_i64().unwrap(),
        digest: output["digest"].as_str().unwrap().to_owned(),
    };
    receipt.digest = "stale-reviewed-input-digest".to_owned();
    serde_json::to_value(vec![receipt]).unwrap()
}
