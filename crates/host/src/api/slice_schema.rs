use crate::tools::object_schema;
use serde_json::{Value, json};

pub(super) fn scope_context() -> Value {
    object_schema(json!({"scope_id":uuid()}), json!(["scope_id"]))
}

pub(super) fn slice_context() -> Value {
    object_schema(json!({"slice_id":uuid()}), json!(["slice_id"]))
}

pub(super) fn candidate_context() -> Value {
    object_schema(
        json!({
            "scope_id":uuid(),
            "view":{"type":"string","enum":["overview","inputs","candidates","reviews","history","results"]},
            "after":{"type":"integer","minimum":0},
            "limit":{"type":"integer","minimum":1,"maximum":100}
        }),
        json!(["scope_id", "view", "limit"]),
    )
}

pub(super) fn open_scope() -> Value {
    object_schema(
        json!({
            "request_id":uuid(),"candidate_set_id":uuid(),
            "candidate_set_revision":{"type":"integer","minimum":1},
            "candidate_snapshot_id":uuid(),"candidate_id":uuid(),
            "candidate_revision":{"type":"integer","minimum":1}
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
                "includes":{"type":"array","items":text(),"maxItems":100},
                "excludes":{"type":"array","items":text(),"maxItems":100},
                "proof":{"type":"array","items":text(),"minItems":1,"maxItems":100},
                "pipeline":{"type":"string","enum":pipelines()},"pipeline_reason":text(),
                "why_lightweight_insufficient":text(),"why_further_vertical_split_not_viable":text()
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
                "input_cursor":{"type":"integer","minimum":0},"request_id":uuid(),payload.0:payload.1
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
    json!({"oneOf":[envelope("draft",("draft",draft)),envelope("review",("review",review))]})
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
        json!({"scope_id":uuid(),"candidate_set_id":uuid(),"revision":{"type":"integer","minimum":1},"request_id":uuid()}),
        json!(["scope_id", "candidate_set_id", "revision", "request_id"]),
    )
}

pub(super) fn open_slice() -> Value {
    object_schema(
        json!({"request_id":uuid(),"scope_id":uuid(),"scope_revision":{"type":"integer","minimum":1},"candidate_set_id":uuid(),"candidate_set_revision":{"type":"integer","minimum":1},"candidate_snapshot_id":uuid(),"candidate_id":uuid(),"candidate_revision":{"type":"integer","minimum":1}}),
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
        "slice.research-to-durable-knowledge",
        "slice.custom-procedure-capture"
    ])
}
fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}
fn text() -> Value {
    json!({"type":"string","minLength":1})
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
