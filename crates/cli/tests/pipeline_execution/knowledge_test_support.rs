use super::recovery_support::Mcp;
use super::support::route;
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) fn enabled() -> bool {
    std::env::var("TECT_TEST_DURABLE_KNOWLEDGE").as_deref() == Ok("1")
}

pub(super) fn draft(binding: Value, statement: &str, source_text: &str) -> Value {
    json!({
        "title":"Exact provenance constraint",
        "statement":statement,
        "modality":"must",
        "action":"retain",
        "target_iri":"urn:tect:target:source-provenance",
        "conditions":["A native Slice phase is active."],
        "exceptions":[],
        "source":{"title":"DK-1 fixture source","uri":"urn:source:dk-1-fixture","text":source_text},
        "binding":binding,
        "purpose":"execution_constraint",
        "version_resolution":"current_accepted"
    })
}

pub(super) fn workspace_draft(statement: &str, source_text: &str) -> Value {
    draft(json!({"kind":"workspace"}), statement, source_text)
}

pub(super) async fn prepare(
    client: &mut Mcp,
    operation: &str,
    generation: i64,
    unit: Option<&Value>,
    revision: Option<i64>,
    proposal: Option<Value>,
) -> (Value, Value) {
    let mut params = json!({
        "request_id":Uuid::new_v4(),"operation":operation,
        "expected_generation":generation,
        "reason":format!("Exercise the exact {operation} lifecycle."),
        "authority_basis":"Authenticated workspace owner fixture."
    });
    if let Some(unit) = unit {
        params["unit_id"] = unit.clone();
    }
    if let Some(revision) = revision {
        params["expected_unit_revision"] = json!(revision);
    }
    if let Some(proposal) = proposal {
        params["draft"] = proposal;
    }
    let result = route(
        client,
        "command",
        "knowledge.change_prepare",
        params.clone(),
    )
    .await;
    (result, params)
}

pub(super) async fn approve(client: &mut Mcp, change: &Value) -> (Value, Value) {
    let params = json!({
        "request_id":Uuid::new_v4(),"change_id":change["id"],
        "change_revision":change["change_revision"],"proposal_digest":change["proposal_digest"],
        "verdict":"approve","review_summary":"Reviewed exact source, modality, conditions, exceptions, binding, authority, and current contradictions.",
        "method_read":{"id":change["review_method"]["id"],
            "version":change["review_method"]["version"],"digest":change["review_method"]["digest"]}
    });
    let result = route(client, "command", "knowledge.change_review", params.clone()).await;
    (result, params)
}

pub(super) async fn publish(client: &mut Mcp, change: &Value) -> (Value, Value) {
    let params = json!({
        "request_id":Uuid::new_v4(),"change_id":change["id"],
        "change_revision":change["change_revision"],"proposal_digest":change["proposal_digest"]
    });
    let result = route(
        client,
        "command",
        "knowledge.change_publish",
        params.clone(),
    )
    .await;
    (result, params)
}
