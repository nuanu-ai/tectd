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
            json!({"provenance":{"type":"string","minLength":1,"maxLength":256}}),
            json!(["provenance"]),
        ));
    }
    branches.push(choice(
        "state",
        "known",
        json!({"value":value,"provenance":{"type":"string","minLength":1,"maxLength":256}}),
        json!(["value", "provenance"]),
    ));
    json!({"oneOf":branches})
}

fn text() -> Value {
    json!({"type":"string","minLength":1,"maxLength":256})
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
        choice("state", "reported", json!({"entries":{"type":"array","minItems":1,"items":object_schema(json!({"name":text(),"fact":fact(text())}), json!(["name","fact"]))}}), json!(["entries"]))
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
    object_schema(
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
    )
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
