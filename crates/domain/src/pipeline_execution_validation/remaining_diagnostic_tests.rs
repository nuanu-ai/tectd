use super::*;
use serde_json::{Value, json};
use uuid::Uuid;

pub(super) fn instruction() -> Value {
    json!({"id":"i","version":"1","digest":"d","body":"body","origin_refs":["source"]})
}
pub(super) fn phase(id: &str, ordinal: u32) -> Value {
    json!({"id":id,"ordinal":ordinal,"title":"Phase","required":true,"disposition_required":false,
    "instructions":[instruction()],"skills":[],"resources":[],"required_fields":[],"allowed_verdicts":["PASS"],"required_dispositions":[],
    "allowed_dispositions":[],"verdict_routes":[route("PASS")],"allowed_backward_to":[],"fresh_reviewer_input":false,"retry_policy":"repeatable","output_contract":"output"})
}
pub(super) fn route(verdict: &str) -> Value {
    json!({"verdict":verdict,"outcome":"completed","transition":"continue","dispositions":[],"revisit_to":[]})
}
pub(super) fn definition() -> Value {
    json!({"kind":"slice.debug-root-cause","version":"0.6","digest":"digest","overview":instruction(),"default_mode":"phasewise","allowed_modes":["phasewise"],
    "phases":[phase("first",1),phase("second",2)],"completion_contract":"complete","escalation_contract":"escalate","forbidden_claims":[]})
}
pub(super) fn parse_definition(value: Value) -> PipelineDefinitionSnapshot {
    serde_json::from_value(value).unwrap()
}
pub(super) fn assert_failure(result: Result<()>, rule: &str, path: &str) {
    let error = result.unwrap_err();
    let refusal = error.refusal().unwrap();
    assert_eq!(refusal.code, RefusalCode::InputSchemaInvalid, "{rule}");
    assert_eq!(refusal.rule.as_deref(), Some(rule));
    assert_eq!(refusal.path.as_deref(), Some(path));
    assert!(refusal.actual.is_some());
    assert!(refusal.expected.is_some());
    assert!(refusal.next_action.is_some());
    assert!(refusal.required.is_some());
}
pub(super) fn set(value: &mut Value, pointer: &str, changed: Value) {
    *value.pointer_mut(pointer).unwrap() = changed;
}
pub(super) fn begin() -> Value {
    json!({"request_id":Uuid::new_v4(),"scope_id":Uuid::new_v4(),"slice_id":Uuid::new_v4(),"slice_revision":1,
    "delivery_mode":null,"definition_version":null,"qualification_reason":"qualified","inquiry":null,"source_checkpoint":null})
}
pub(super) fn inquiry() -> Value {
    json!({"topic_level":"slice","task_context":{},"completion":{"kind":"research","allow_inconclusive":false}})
}
pub(super) fn amendment() -> Value {
    json!({"target_phase_id":"target","predecessor":{"output_id":Uuid::new_v4(),"output_revision":1,"output_digest":"old",
    "artifact_name":"../old name","artifact_digest":"old","source_path":"source.md","source_digest":"old"},
    "successor":{"path":"source.md","artifact":{"name":"source.md","media_type":"text/markdown","body":"body","digest":"a".repeat(64),"reference":null}},
    "authorization_scope":"scope","authorization_provenance":"provenance"})
}
pub(super) fn input() -> Value {
    json!({"request_id":Uuid::new_v4(),"run_id":Uuid::new_v4(),"run_revision":1,"phase_id":"phase","input":"input","source_amendment":null})
}

mod amendments;
mod definition;
mod order;
mod requests;
pub(super) fn route_definition() -> Value {
    let mut value = definition();
    value["phases"][1]["allowed_verdicts"] = json!(["PASS", "OTHER"]);
    value["phases"][1]["verdict_routes"] = json!([route("PASS"), route("OTHER")]);
    value
}
