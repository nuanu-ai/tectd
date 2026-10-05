use super::*;
use crate::pipeline_tools::PipelineInvocation;
use tect_domain::{
    BeginPipelineRun, CompletePipelinePhase, PipelineDefinitionSnapshot, RecordPipelineInput,
};
use uuid::Uuid;

fn instruction() -> Value {
    json!({"id":"i","version":"1","digest":"d","body":"PRIVATE_INSTRUCTION_BODY","origin_refs":["PRIVATE_ORIGIN"]})
}
fn definition() -> Value {
    let phase = |id, ordinal| json!({"id":id,"ordinal":ordinal,"title":"Phase","required":true,"disposition_required":false,"instructions":[instruction()],"skills":[],"resources":[],"required_fields":[],"allowed_verdicts":["PASS","OTHER"],"required_dispositions":[],"allowed_dispositions":[],"verdict_routes":[{"verdict":"PASS","outcome":"completed","transition":"continue","dispositions":[],"revisit_to":[]},{"verdict":"OTHER","outcome":"completed","transition":"continue","dispositions":[],"revisit_to":[]}],"allowed_backward_to":[],"fresh_reviewer_input":false,"retry_policy":"repeatable","output_contract":"output"});
    json!({"kind":"slice.debug-root-cause","version":"0.6","digest":"digest","overview":instruction(),"default_mode":"phasewise","allowed_modes":["phasewise"],"phases":[phase("first",1),phase("second",2)],"completion_contract":"complete","escalation_contract":"escalate","forbidden_claims":[]})
}
fn assert_full_failure(
    error: Error,
    invocation: PipelineInvocation,
    route: &str,
    params: Value,
    rule: &str,
    path: &str,
    output_error: bool,
) {
    let boundary = invocation.refusal_boundary();
    let normalized = error.normalize_pipeline_refusal(
        boundary.rule,
        boundary.path,
        boundary.expected,
        boundary.next_action,
        boundary.required,
    );
    let original = normalized.refusal().unwrap();
    assert_eq!(original.rule.as_deref(), Some(rule));
    assert_eq!(original.path.as_deref(), Some(path));
    let arguments = json!({"route":route,"params":params});
    crate::api::decode_public_call("command", arguments.clone()).unwrap();
    let response = failure_response(
        json!(7),
        responses::failure_bounded(normalized, Some(("command", &arguments)), None, 8192),
        8192,
    );
    let bytes = serde_json::to_vec(&response).unwrap();
    assert!(bytes.len() <= 8192);
    assert_eq!(response["result"]["isError"], true);
    let content = &response["result"]["content"];
    let data: Value = serde_json::from_str(content[1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(data["error"]["refusal"]["rule"], rule);
    assert_eq!(data["error"]["refusal"]["path"], path);
    assert_eq!(
        content[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("The submitted output"),
        output_error
    );
    let wire = String::from_utf8(bytes).unwrap();
    for private in [
        "PRIVATE_INSTRUCTION_BODY",
        "PRIVATE_ORIGIN",
        "PRIVATE_ARTIFACT_BODY",
        "PRIVATE_AUTHORIZATION_PROVENANCE",
    ] {
        assert!(!wire.contains(private), "source material echoed: {private}");
    }
    println!("{rule} full_mcp_bytes={}", wire.len());
}
fn begin() -> Value {
    json!({"request_id":Uuid::new_v4(),"scope_id":Uuid::new_v4(),"slice_id":Uuid::new_v4(),"slice_revision":1,"qualification_reason":"qualified"})
}
#[test]
fn real_p7_definition_indexed_instruction_and_route_errors_survive_full_mcp() {
    let request: BeginPipelineRun = serde_json::from_value(begin()).unwrap();
    for (pointer, changed, rule, path) in [
        (
            "/phases/1/instructions/1/id",
            json!(" "),
            "WP6-INSTRUCTION-ID",
            "pipeline_definition.phases[1].instructions[1].id",
        ),
        (
            "/phases/1/verdict_routes/1/dispositions",
            json!([" "]),
            "WP6-PHASE-ROUTE-DISPOSITION-BLANK",
            "pipeline_definition.phases[1].verdict_routes[1].dispositions[0]",
        ),
    ] {
        let mut value = definition();
        value["phases"][1]["instructions"] = json!([instruction(), instruction()]);
        *value.pointer_mut(pointer).unwrap() = changed;
        let definition: PipelineDefinitionSnapshot = serde_json::from_value(value).unwrap();
        let error = definition.validate().unwrap_err();
        assert_full_failure(
            error,
            PipelineInvocation::Begin(request.clone()),
            "slice.pipeline.begin",
            begin(),
            rule,
            path,
            false,
        );
    }
}
#[test]
fn real_p7_begin_and_source_amendment_errors_survive_full_mcp_without_material_echo() {
    let definition: PipelineDefinitionSnapshot = serde_json::from_value(definition()).unwrap();
    let mut params = begin();
    params["request_id"] = json!(Uuid::nil());
    let request: BeginPipelineRun = serde_json::from_value(params.clone()).unwrap();
    assert_full_failure(
        request.validate(&definition).unwrap_err(),
        PipelineInvocation::Begin(request),
        "slice.pipeline.begin",
        params,
        "WP6-BEGIN-REQUEST-ID",
        "arguments.params.request_id",
        false,
    );
    let id = Uuid::new_v4();
    let params = json!({"request_id":id,"run_id":id,"run_revision":1,"phase_id":"phase","input":"input","source_amendment":{"target_phase_id":"target","predecessor":{"output_id":id,"output_revision":1,"output_digest":"old","artifact_name":"old","artifact_digest":"old","source_path":"source.md","source_digest":"old"},"successor":{"path":"source.md","artifact":{"name":"source.md","media_type":"text/markdown","body":"PRIVATE_ARTIFACT_BODY".repeat(110000),"digest":"a".repeat(64)}},"authorization_scope":"scope","authorization_provenance":"PRIVATE_AUTHORIZATION_PROVENANCE"}});
    let request: RecordPipelineInput = serde_json::from_value(params.clone()).unwrap();
    assert_full_failure(
        request.validate().unwrap_err(),
        PipelineInvocation::Input(Box::new(request)),
        "slice.pipeline.input",
        params,
        "WP6-SOURCE-AMENDMENT-BODY-SIZE",
        "arguments.params.source_amendment.successor.artifact.body",
        false,
    );
}
#[test]
fn real_domain_output_error_uses_output_intro_after_same_boundary_normalization() {
    let definition: PipelineDefinitionSnapshot = serde_json::from_value(definition()).unwrap();
    let id = Uuid::new_v4();
    let params = json!({"request_id":id,"run_id":id,"run_revision":1,"phase_id":"first","outcome":"completed","transition":"continue","output":{"producer_context_id":"actor","body":""}});
    let request: CompletePipelinePhase = serde_json::from_value(params.clone()).unwrap();
    assert_full_failure(
        request.validate(&definition).unwrap_err(),
        PipelineInvocation::Complete(Box::new(request)),
        "slice.pipeline.phase.complete",
        params,
        "WP6-COMPLETE-OUTPUT-01",
        "arguments.params.output.body",
        true,
    );
}
