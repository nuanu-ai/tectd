use crate::tools::object_schema;
use serde_json::{Value, json};

fn choice(tag: &str, variant: &str, properties: Value, required: Value) -> Value {
    let mut properties = properties.as_object().cloned().unwrap();
    properties.insert(tag.into(), json!({"const":variant}));
    let mut required = required.as_array().cloned().unwrap();
    required.push(json!(tag));
    object_schema(Value::Object(properties), Value::Array(required))
}

fn fact(value: Value) -> Value {
    let mut branches = vec![choice("state", "absent", json!({}), json!([]))];
    for state in ["known_empty", "unknown", "gap", "conflict", "invalid"] {
        branches.push(choice(
            "state",
            state,
            json!({"provenance":text()}),
            json!(["provenance"]),
        ));
    }
    branches.push(choice(
        "state",
        "known",
        json!({"value":value,"provenance":text()}),
        json!(["value", "provenance"]),
    ));
    json!({"oneOf":branches})
}

fn text() -> Value {
    json!({
        "type":"string",
        "minLength":1,
        "maxLength":256,
        "pattern":"\\S",
        "description":"Nonblank after Unicode trimming; at most 256 UTF-8 bytes. Host validation enforces the byte limit.",
        "x-maxUtf8Bytes":256
    })
}

fn bounded_text(max_bytes: usize) -> Value {
    json!({
        "type":"string", "minLength":1, "maxLength":max_bytes,
        "pattern":"\\S", "x-maxUtf8Bytes":max_bytes,
        "description":format!("Nonblank, no control characters, at most {max_bytes} UTF-8 bytes; host validation enforces byte and control limits.")
    })
}

fn opaque_id() -> Value {
    json!({
        "type":"string", "minLength":1, "maxLength":256,
        "pattern":"^\\S+$", "x-maxUtf8Bytes":256,
        "description":"Opaque nonblank ID without whitespace or control characters; host validation enforces the 256 UTF-8 byte limit."
    })
}

pub(super) fn choice_set() -> Value {
    let mut schema = object_schema(
        json!({
            "schema":{"const":"tect.matrix-choice-set/1"},
            "choice_set_id":opaque_id(),
            "version":{"type":"integer","minimum":1},
            "task_id":opaque_id(),
            "task_revision":opaque_id(),
            "decision_question":bounded_text(1024),
            "candidates":{
                "type":"array", "maxItems":5,
                "items":object_schema(json!({
                    "candidate_id":opaque_id(),
                    "title":bounded_text(256),
                    "approach":bounded_text(4096),
                    "assumption_fact_ids":{"type":"array","uniqueItems":true,"items":opaque_id()}
                }),json!(["candidate_id","title","approach","assumption_fact_ids"]))
            }
        }),
        json!([
            "schema",
            "choice_set_id",
            "version",
            "task_id",
            "task_revision",
            "decision_question",
            "candidates"
        ]),
    );
    schema["description"] = json!(
        "Optional owner-authored alternatives. Zero or one candidate remains stored but is ineligible for ranking. Assumption IDs must reference facts in the same Matrix input. The combined input and choice-set JSON is limited to 1 MiB."
    );
    schema
}

fn intent() -> Value {
    json!({"oneOf":[
        choice("kind", "production_hotfix", json!({}), json!([])),
        choice("kind", "other", json!({"description":text()}), json!(["description"]))
    ]})
}

fn operational_facts() -> Value {
    json!({"oneOf":[
        choice("state", "absent", json!({}), json!([])),
        choice("state", "known_empty", json!({"provenance":text()}), json!(["provenance"])),
        choice("state", "reported", json!({"entries":{"type":"array","minItems":1,"maxItems":1024,"items":object_schema(json!({"name":text(),"fact":fact(text())}), json!(["name","fact"]))}}), json!(["entries"]))
    ]})
}

pub(super) fn input() -> Value {
    let keys = [
        "mode",
        "envelope",
        "criticality",
        "intent",
        "urgency",
        "promised_behavior",
        "promised_proof",
        "affected_guarantees",
        "actual_exposure",
        "demand_commitment",
        "latency_commitment",
        "urgent_repair",
    ];
    let mut schema = object_schema(
        json!({
            "mode":fact(json!({"type":"string","enum":["demo","mvp","production"]})),
            "envelope":object_schema(json!({"scale":fact(text()),"operational_facts":operational_facts()}), json!(["scale","operational_facts"])),
            "criticality":fact(text()),
            "intent":fact(intent()),
            "urgency":fact(text()),
            "promised_behavior":fact(text()),
            "promised_proof":fact(text()),
            "affected_guarantees":fact(json!({"type":"array","minItems":1,"uniqueItems":true,"items":{"type":"string","enum":["payment","secret","data"]}})),
            "actual_exposure":fact(json!({"type":"boolean"})),
            "demand_commitment":fact(json!({"type":"string","enum":["no_commitment","within_verified_limit","lacks_evidence","exceeds_verified_limit"]})),
            "latency_commitment":fact(json!({"type":"string","enum":["no_commitment","within_verified_limit","lacks_evidence","exceeds_verified_limit"]})),
            "urgent_repair":fact(json!({"type":"boolean"}))
        }),
        json!(keys),
    );
    schema["description"] = json!(
        "Tagged factual input; combined input and optional choice-set serialized JSON is capped at 1 MiB by the host."
    );
    schema["x-maxSerializedJsonBytes"] = json!(1024 * 1024);
    schema
}

pub(super) fn example() -> Value {
    let absent = json!({"state":"absent"});
    json!({
        "mode":absent,"envelope":{"scale":absent,"operational_facts":{"state":"absent"}},
        "criticality":absent,"intent":absent,"urgency":absent,
        "promised_behavior":absent,"promised_proof":absent,"affected_guarantees":absent,
        "actual_exposure":absent,"demand_commitment":absent,"latency_commitment":absent,
        "urgent_repair":absent
    })
}

pub(super) fn example_choice_set(task_id: &str) -> Value {
    json!({
        "schema":"tect.matrix-choice-set/1",
        "choice_set_id":"implementation-options",
        "version":1,
        "task_id":task_id,
        "task_revision":"1",
        "decision_question":"Which implementation approach should we evaluate?",
        "candidates":[
            {"candidate_id":"approach-a","title":"First approach","approach":"Use the first approach.","assumption_fact_ids":["mode"]},
            {"candidate_id":"approach-b","title":"Second approach","approach":"Use the second approach.","assumption_fact_ids":["envelope.scale"]}
        ]
    })
}
