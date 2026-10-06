use crate::tools::object_schema;
use serde_json::{Value, json};

mod pipeline;
pub(super) use pipeline::{
    pipeline_begin, pipeline_checkpoint_resolve, pipeline_context, pipeline_delivery_escalate,
    pipeline_input, pipeline_instruction, pipeline_phase_complete, pipeline_run_migrate,
    scope_context, slice_context,
};

pub(super) fn candidate_context() -> Value {
    crate::planning_read::schema(object_schema(
        json!({
            "scope_id":uuid(),
            "view":{"type":"string","enum":["overview","details","inputs","candidates","reviews","history","results"]},
            "after":{"type":"integer","minimum":0},
            "limit":{"type":"integer","minimum":1,"maximum":100}
        }),
        json!(["scope_id", "view", "limit"]),
    ))
}

pub(super) fn open_scope() -> Value {
    object_schema(
        json!({
            "request_id":uuid(),"candidate_set_id":uuid(),
            "candidate_set_revision":{"type":"integer","minimum":1},
            "candidate_snapshot_id":uuid(),"candidate_id":uuid(),
            "candidate_revision":{"type":"integer","minimum":1},
            "task_context":super::planning_task_context(),
            "consumed_knowledge":super::planning_manifest_guard()
        }),
        json!([
            "request_id",
            "candidate_set_id",
            "candidate_set_revision",
            "candidate_snapshot_id",
            "candidate_id",
            "candidate_revision"
        ]),
    )
}

pub(super) fn open_scope_example() -> Value {
    let id = "00000000-0000-4000-8000-000000000001";
    json!({"request_id":id,"candidate_set_id":id,"candidate_set_revision":3,"candidate_snapshot_id":id,"candidate_id":id,"candidate_revision":1})
}

pub(super) fn save() -> Value {
    let identity = json!({"oneOf":[
        object_schema(json!({"local":local()}),json!(["local"])),
        object_schema(json!({"candidate_id":uuid(),"revision":{"type":"integer","minimum":1}}),json!(["candidate_id","revision"]))
    ]});
    let reference = json!({"oneOf":[
        object_schema(json!({"local":local()}),json!(["local"])),
        object_schema(json!({"candidate_id":uuid(),"revision":{"type":"integer","minimum":1}}),json!(["candidate_id","revision"]))
    ]});
    let common = json!({
        "identity":identity,"change_rationale":text(),"title":text(),
        "dependencies":{"type":"array","items":reference,"maxItems":100},
        "source_result_ids":{"type":"array","items":uuid(),"maxItems":100,"uniqueItems":true}
    });
    let work = object_schema(
        merge(
            common.clone(),
            json!({
                "kind":{"const":"work"},"outcome":text(),
                "model_route_facts":model_route_facts(),
                "includes":{"type":"array","items":text(),"maxItems":100},
                "excludes":{"type":"array","items":text(),"maxItems":100},
                "proof":{"type":"array","items":text(),"minItems":1,"maxItems":100},
                "pipeline":{"type":"string","enum":pipelines()},"pipeline_reason":text(),
                "why_lightweight_insufficient":text(),"why_further_vertical_split_not_viable":text()
                ,"source_checkpoint":checkpoint_ref()
            }),
        ),
        json!([
            "kind",
            "identity",
            "title",
            "outcome",
            "proof",
            "pipeline",
            "pipeline_reason"
        ]),
    );
    let decision = object_schema(
        merge(
            common,
            json!({
                "kind":{"const":"decision"},"question":text(),
                "resolution_criteria":{"type":"array","items":text(),"minItems":1,"maxItems":100}
            }),
        ),
        json!([
            "kind",
            "identity",
            "title",
            "question",
            "resolution_criteria"
        ]),
    );
    let supersession = object_schema(
        json!({
            "candidate_id":uuid(),"revision":{"type":"integer","minimum":1},"reason":text(),
            "replacements":{"type":"array","items":reference,"maxItems":100},
            "source_result_ids":{"type":"array","items":uuid(),"maxItems":100,"uniqueItems":true,"default":[]}
        }),
        json!(["candidate_id", "revision", "reason"]),
    );
    let draft = object_schema(
        json!({
            "coverage_summary":text(),"nodes":{"type":"array","items":{"oneOf":[work,decision]},"minItems":1,"maxItems":100},
            "supersessions":{"type":"array","items":supersession,"maxItems":100}
        }),
        json!(["coverage_summary", "nodes"]),
    );
    let finding = object_schema(
        json!({
            "material":{"type":"boolean"},"summary":text(),
            "candidate_ids":{"type":"array","items":uuid(),"maxItems":100,"uniqueItems":true},"disposition":text()
        }),
        json!(["material", "summary", "disposition"]),
    );
    let review = object_schema(
        json!({
            "verdict":{"type":"string","enum":["ready","revise","blocked"]},"summary":text(),
            "findings":{"type":"array","items":finding,"maxItems":100}
        }),
        json!(["verdict", "summary"]),
    );
    let envelope = |kind: &str, payload: (&str, Value)| {
        object_schema(
            json!({
                "kind":{"const":kind},"scope_id":uuid(),"candidate_set_id":uuid(),
                "revision":{"type":"integer","minimum":1},"snapshot_id":uuid(),
                "input_cursor":{"type":"integer","minimum":0},"request_id":uuid(),
                "consumed_knowledge":super::planning_manifest_guard(),payload.0:payload.1
            }),
            json!([
                "kind",
                "scope_id",
                "candidate_set_id",
                "revision",
                "snapshot_id",
                "input_cursor",
                "request_id",
                payload.0
            ]),
        )
    };
    let mut draft_envelope = envelope("draft", ("draft", draft));
    draft_envelope["properties"]
        .as_object_mut()
        .unwrap()
        .insert("matrix_selection".into(), matrix_selection());
    json!({"oneOf":[draft_envelope,envelope("review",("review",review))]})
}

pub(super) fn save_example() -> Value {
    let id = "00000000-0000-4000-8000-000000000001";
    json!({"kind":"draft","scope_id":id,"candidate_set_id":id,"revision":1,"snapshot_id":id,"input_cursor":1,"request_id":id,
        "draft":{"coverage_summary":"The complete Scope is represented.","nodes":[{"kind":"work","identity":{"local":"first"},"title":"Deliver bounded behavior","outcome":"The behavior is observable.","proof":["Direct acceptance evidence"],"pipeline":"slice.lightweight-tdd-development","pipeline_reason":"Bounded development is sufficient."}]}})
}

pub(super) fn record_input() -> Value {
    object_schema(
        json!({"scope_id":uuid(),"candidate_set_id":uuid(),"revision":{"type":"integer","minimum":1},"request_id":uuid(),"input":text()}),
        json!([
            "scope_id",
            "candidate_set_id",
            "revision",
            "request_id",
            "input"
        ]),
    )
}

pub(super) fn refresh() -> Value {
    object_schema(
        json!({"scope_id":uuid(),"candidate_set_id":uuid(),"revision":{"type":"integer","minimum":1},"request_id":uuid(),"task_context":super::planning_task_context()}),
        json!(["scope_id", "candidate_set_id", "revision", "request_id"]),
    )
}

pub(super) fn open_slice() -> Value {
    object_schema(
        json!({"request_id":uuid(),"scope_id":uuid(),"scope_revision":{"type":"integer","minimum":1},"candidate_set_id":uuid(),"candidate_set_revision":{"type":"integer","minimum":1},"candidate_snapshot_id":uuid(),"candidate_id":uuid(),"candidate_revision":{"type":"integer","minimum":1},"disposition_id":uuid()}),
        json!([
            "request_id",
            "scope_id",
            "scope_revision",
            "candidate_set_id",
            "candidate_set_revision",
            "candidate_snapshot_id",
            "candidate_id",
            "candidate_revision"
        ]),
    )
}

pub(super) fn open_slice_example() -> Value {
    let id = "00000000-0000-4000-8000-000000000001";
    json!({"request_id":id,"scope_id":id,"scope_revision":1,"candidate_set_id":id,"candidate_set_revision":3,"candidate_snapshot_id":id,"candidate_id":id,"candidate_revision":1})
}

pub(super) fn record_result() -> Value {
    let evidence = object_schema(
        json!({"kind":text(),"reference":text(),"observation":text()}),
        json!(["kind", "reference", "observation"]),
    );
    object_schema(
        json!({"request_id":uuid(),"scope_id":uuid(),"slice_id":uuid(),"slice_revision":{"type":"integer","minimum":1},"outcome":{"type":"string","enum":["completed","blocked"]},"summary":text(),"evidence":{"type":"array","items":evidence,"minItems":1,"maxItems":100},"scope_impact":text(),"remaining_work":text()}),
        json!([
            "request_id",
            "scope_id",
            "slice_id",
            "slice_revision",
            "outcome",
            "summary",
            "evidence",
            "scope_impact",
            "remaining_work"
        ]),
    )
}

pub(super) fn record_result_example() -> Value {
    let id = "00000000-0000-4000-8000-000000000001";
    json!({"request_id":id,"scope_id":id,"slice_id":id,"slice_revision":1,"outcome":"completed","summary":"Bounded outcome observed.","evidence":[{"kind":"test","reference":"test name","observation":"Acceptance passed."}],"scope_impact":"One planned uncertainty is resolved.","remaining_work":"Refresh and review affected future candidates."})
}

fn pipelines() -> Value {
    json!([
        "slice.lightweight-tdd-development",
        "slice.full-design-to-execution",
        "slice.debug-root-cause",
        "slice.operational-preparation",
        "slice.operational-execution",
        "slice.research",
        "slice.deep-brainstorming",
        "slice.research-to-durable-knowledge",
        "slice.custom-procedure-capture",
        "slice.promote-to-durable-knowledge"
    ])
}
fn checkpoint_ref() -> Value {
    object_schema(
        json!({"checkpoint_id":uuid(),"digest":text()}),
        json!(["checkpoint_id", "digest"]),
    )
}
fn inquiry() -> Value {
    let research = object_schema(
        json!({"kind":{"const":"research"},"allow_inconclusive":{"type":"boolean"}}),
        json!(["kind", "allow_inconclusive"]),
    );
    let decision = object_schema(
        json!({"kind":{"const":"decision"},"requested_outcome":{"type":"string","enum":["decision","recommendation"]}}),
        json!(["kind", "requested_outcome"]),
    );
    object_schema(
        json!({"topic_level":{"type":"string","enum":["program","scope","slice"]},
            "task_context":super::planning_task_context(),
            "completion":{"oneOf":[research,decision]}}),
        json!(["topic_level", "task_context", "completion"]),
    )
}
fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}
fn text() -> Value {
    json!({"type":"string","minLength":1})
}
fn context_id() -> Value {
    json!({"type":"string","minLength":1,"maxLength":1024})
}
fn local() -> Value {
    json!({"type":"string","minLength":1,"maxLength":64,"pattern":"^[A-Za-z0-9._-]+$"})
}
fn merge(mut left: Value, right: Value) -> Value {
    left.as_object_mut()
        .unwrap()
        .extend(right.as_object().unwrap().clone());
    left
}

fn model_route_facts() -> Value {
    let label =
        json!({"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9._/:\\-]+$"});
    json!({"type":"object","additionalProperties":false,"minProperties":1,"properties":{
        "role":label,"tool":label,"data_class":label,
        "remaining_budget_units":{"type":"integer","minimum":0,"maximum":18446744073709551615u64},
        "available_latency_ms":{"type":"integer","minimum":0,"maximum":18446744073709551615u64}
    },"description":"Caller-authored route constraints for this Work revision; independently verified evidence is separate."})
}

fn matrix_selection() -> Value {
    let digest = json!({"type":"string","minLength":64,"maxLength":64,"pattern":"^[0-9a-f]{64}$"});
    object_schema(
        json!({
            "task_id":uuid(),"task_revision":{"type":"integer","minimum":1,"maximum":i64::MAX},
            "disposition_id":uuid(),"selected_choice_id":{"type":"string","minLength":1,"maxLength":4096},
            "expected_input_digest":digest,"expected_choice_set_digest":digest,"expected_verification_digest":digest,
            "mapped_draft_node_indices":{"type":"array","items":{"type":"integer","minimum":0,"maximum":u64::MAX},"minItems":1,"maxItems":100,"uniqueItems":true,"description":"Strictly increasing draft node positions attributed to the selected choice."}
        }),
        json!([
            "task_id",
            "task_revision",
            "disposition_id",
            "selected_choice_id",
            "expected_input_digest",
            "expected_choice_set_digest",
            "expected_verification_digest",
            "mapped_draft_node_indices"
        ]),
    )
}

#[cfg(test)]
mod s05_schema_tests {
    use super::*;

    #[test]
    fn s05_work_model_route_schema_and_strict_decoder_match() {
        let schema = save();
        let route_facts = &schema["oneOf"][0]["properties"]["draft"]["properties"]["nodes"]["items"]
            ["oneOf"][0]["properties"]["model_route_facts"];
        assert_eq!(route_facts["additionalProperties"], false);
        assert_eq!(route_facts["minProperties"], 1);
        let keys: std::collections::BTreeSet<_> = route_facts["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::BTreeSet::from([
                "role",
                "tool",
                "data_class",
                "remaining_budget_units",
                "available_latency_ms"
            ])
        );
        let mut example = save_example();
        example["draft"]["nodes"][0]["model_route_facts"] = json!({"role":"agent","tool":"code","data_class":"internal","remaining_budget_units":u64::MAX,"available_latency_ms":u64::MAX});
        assert!(
            crate::api::decode_public_call(
                "command",
                json!({"route":"slice.candidates.save","params":example})
            )
            .is_ok()
        );
        example["draft"]["nodes"][0]["model_route_facts"]["forged_authority"] = json!("accepted");
        assert!(
            crate::api::decode_public_call(
                "command",
                json!({"route":"slice.candidates.save","params":example})
            )
            .is_err()
        );
        let invalid: tect_domain::ModelRouteCallerFacts =
            serde_json::from_value(json!({"role":" agent"})).unwrap();
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn s05_selected_draft_schema_matches_exact_typed_selection() {
        let schema = save();
        let selection = &schema["oneOf"][0]["properties"]["matrix_selection"];
        assert_eq!(selection["additionalProperties"], false);
        assert_eq!(selection["required"].as_array().unwrap().len(), 8);
        assert!(
            schema["oneOf"][1]["properties"]
                .get("matrix_selection")
                .is_none()
        );
        let mut example = save_example();
        let id = "00000000-0000-4000-8000-000000000001";
        example["matrix_selection"] = json!({"task_id":id,"task_revision":1,"disposition_id":id,"selected_choice_id":"owner-choice","expected_input_digest":"a".repeat(64),"expected_choice_set_digest":"b".repeat(64),"expected_verification_digest":"c".repeat(64),"mapped_draft_node_indices":[0]});
        assert!(
            crate::api::decode_public_call(
                "command",
                json!({"route":"slice.candidates.save","params":example})
            )
            .is_ok()
        );
        example["matrix_selection"]["forged_authority"] = json!("accepted");
        assert!(
            crate::api::decode_public_call(
                "command",
                json!({"route":"slice.candidates.save","params":example})
            )
            .is_err()
        );
        example["matrix_selection"]
            .as_object_mut()
            .unwrap()
            .remove("forged_authority");
        example["matrix_selection"]
            .as_object_mut()
            .unwrap()
            .remove("expected_verification_digest");
        assert!(
            crate::api::decode_public_call(
                "command",
                json!({"route":"slice.candidates.save","params":example})
            )
            .is_err()
        );
    }
}
