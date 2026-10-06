use super::{Mcp, ResolvedPipeline, Value, json, resolve_pipeline, route};

pub(super) fn assert_disposition(raw: &Value, outcome: &str) {
    if let Some(value) = raw.get(outcome) {
        assert!(value.is_object(), "actual inline disposition required");
    } else {
        assert_eq!(raw["outcome"], outcome);
        assert_eq!(raw["changed"], outcome != "replay");
    }
}

pub(super) fn constraint(source_text: &str) -> Value {
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    fixture["document"]["canonical_text"] =
        json!("Every active phase must retain exact source provenance.");
    fixture["document"]["sources"][0]["snapshot"]["text"] = json!(source_text);
    fixture["document"].clone()
}

pub(super) async fn read_current(client: &mut Mcp, params: Value) -> ResolvedPipeline {
    let raw = route(client, "query", "slice.pipeline.context", params).await;
    resolve_pipeline(client, raw).await.unwrap()
}
pub(super) fn assert_compact(context: &ResolvedPipeline) {
    assert!(
        context.compact_context["definition"]
            .get("phases")
            .is_none()
    );
}
pub(super) fn assert_state(context: &ResolvedPipeline, state: &str) {
    assert_eq!(
        context.details_data()["knowledge_resource_status"]["state"],
        state
    );
}
