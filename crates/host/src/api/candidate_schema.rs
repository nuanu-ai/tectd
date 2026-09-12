use crate::tools::object_schema;
use serde_json::{Value, json};

pub(super) fn context() -> Value {
    let page = |view| {
        object_schema(
            json!({
                "candidate_set_id":uuid(),"view":{"const":view},
                "after":{"type":"integer","minimum":0},
                "limit":{"type":"integer","minimum":1,"maximum":100}
            }),
            json!(["candidate_set_id", "view", "limit"]),
        )
    };
    json!({"oneOf":[
        page("overview"),page("program"),page("inputs"),page("candidates"),page("reviews"),
        object_schema(
            json!({
                "candidate_set_id":uuid(),"view":{"const":"fragment"},
                "source_ref_id":uuid(),"cursor":{"type":"integer","minimum":0}
            }),
            json!(["candidate_set_id","view","source_ref_id","cursor"])
        )
    ]})
}

pub(super) fn begin() -> Value {
    object_schema(
        json!({
            "request_id":uuid(),
            "program_id":uuid(),
            "program_revision":{"type":"integer","minimum":1},
            "boundary":{"type":"string","enum":["finite","ongoing"]},
            "input":text()
        }),
        json!([
            "request_id",
            "program_id",
            "program_revision",
            "boundary",
            "input"
        ]),
    )
}

pub(super) fn save() -> Value {
    let identity = json!({
        "oneOf":[
            object_schema(json!({"local":local()}), json!(["local"])),
            object_schema(
                json!({"id":uuid(),"revision":{"type":"integer","minimum":1}}),
                json!(["id","revision"])
            )
        ]
    });
    let reference = json!({
        "oneOf":[
            object_schema(json!({"local":local()}), json!(["local"])),
            object_schema(json!({"id":uuid()}), json!(["id"]))
        ]
    });
    let goal = object_schema(
        json!({
            "identity":identity,
            "text":text(),
            "source_ref_id":uuid(),
            "exact_quote":{"type":"string"},
            "resolution":object_schema(
                json!({
                    "kind":{"type":"string","enum":["candidate","evidence","blocker"]},
                    "reference":reference
                }),
                json!(["kind","reference"])
            )
        }),
        json!(["identity", "text", "source_ref_id", "resolution"]),
    );
    let evidence = object_schema(
        json!({
            "identity":identity,
            "kind":{"type":"string","enum":["verified_evidence","accepted_work"]},
            "summary":text(),
            "source_ref_id":uuid(),
            "authority_input_sequence":{"type":"integer","minimum":1}
        }),
        json!(["identity", "kind", "summary", "source_ref_id"]),
    );
    let candidate = object_schema(
        json!({
            "identity":identity,
            "title":text(),
            "outcome":text(),
            "trigger":text(),
            "delivered_behavior":text(),
            "proof":text(),
            "includes":{"type":"array","items":text(),"maxItems":100},
            "excludes":{"type":"array","items":text(),"maxItems":100},
            "dependencies":{"type":"array","items":reference,"maxItems":100},
            "coverage_goals":{"type":"array","items":reference,"minItems":1,"maxItems":100},
            "evidence":{"type":"array","items":reference,"maxItems":100}
        }),
        json!([
            "identity",
            "title",
            "outcome",
            "trigger",
            "delivered_behavior",
            "proof",
            "coverage_goals"
        ]),
    );
    let blocker = object_schema(
        json!({"identity":identity,"summary":text(),"source_ref_id":uuid()}),
        json!(["identity", "summary", "source_ref_id"]),
    );
    let empty_disposition = object_schema(
        json!({
            "kind":{"type":"string","enum":["all_covered","out_of_boundary","needs_input"]},
            "reason":text(),
            "source_ref_id":uuid()
        }),
        json!(["kind", "reason", "source_ref_id"]),
    );
    let protected_change = object_schema(
        json!({
            "accepted_evidence_id":uuid(),"prior_candidate_id":uuid(),
            "disposition":{"type":"string","enum":["delete","replace","reassociate"]},
            "rationale":text(),"authority_source_ref_id":uuid(),
            "replacement_evidence":reference,"target_candidate":reference
        }),
        json!([
            "accepted_evidence_id",
            "disposition",
            "rationale",
            "authority_source_ref_id"
        ]),
    );
    let draft = object_schema(
        json!({
            "boundary":{"type":"string","enum":["finite","ongoing"]},
            "goals":{"type":"array","items":goal,"maxItems":100},
            "evidence":{"type":"array","items":evidence,"maxItems":100},
            "candidates":{"type":"array","items":candidate,"maxItems":100},
            "blockers":{"type":"array","items":blocker,"maxItems":100},
            "pending_question":{"type":"string"},
            "empty_disposition":empty_disposition,
            "protected_changes":{"type":"array","items":protected_change,"maxItems":100}
        }),
        json!(["boundary", "goals", "candidates"]),
    );
    let finding = object_schema(
        json!({
            "severity":{"type":"string","enum":["advisory","material"]},
            "summary":text(),
            "candidate_ids":{"type":"array","items":uuid(),"maxItems":100,"uniqueItems":true},
            "coverage_goal_ids":{"type":"array","items":uuid(),"maxItems":100,"uniqueItems":true},
            "disposition":text()
        }),
        json!(["severity", "summary", "disposition"]),
    );
    let decision = object_schema(
        json!({
            "candidate_id":uuid(),
            "decision":{"type":"string","enum":["accept","revise","reject"]},
            "rationale":text()
        }),
        json!(["candidate_id", "decision", "rationale"]),
    );
    let review = object_schema(
        json!({
            "verdict":{"type":"string","enum":["ready","revise","blocked"]},
            "summary":text(),
            "findings":{"type":"array","items":finding,"maxItems":100},
            "candidate_decisions":{"type":"array","items":decision,"maxItems":100},
            "protected_change_reviews":{"type":"array","items":object_schema(
                json!({"accepted_evidence_id":uuid(),"prior_candidate_id":uuid(),"rationale":text()}),
                json!(["accepted_evidence_id","rationale"])
            ),"maxItems":100}
        }),
        json!(["verdict", "summary", "findings", "candidate_decisions"]),
    );
    json!({
        "oneOf":[
            object_schema(
                json!({
                    "kind":{"const":"draft"},
                    "candidate_set_id":uuid(),
                    "revision":{"type":"integer","minimum":1},
                    "snapshot_id":uuid(),
                    "input_cursor":{"type":"integer","minimum":0},
                    "request_id":uuid(),
                    "draft":draft
                }),
                json!(["kind","candidate_set_id","revision","snapshot_id","input_cursor","request_id","draft"])
            ),
            object_schema(
                json!({
                    "kind":{"const":"review"},
                    "candidate_set_id":uuid(),
                    "revision":{"type":"integer","minimum":1},
                    "snapshot_id":uuid(),
                    "input_cursor":{"type":"integer","minimum":0},
                    "request_id":uuid(),
                    "review":review
                }),
                json!(["kind","candidate_set_id","revision","snapshot_id","input_cursor","request_id","review"])
            )
        ]
    })
}

pub(super) fn record_input() -> Value {
    object_schema(
        json!({
            "candidate_set_id":uuid(),
            "revision":{"type":"integer","minimum":1},
            "request_id":uuid(),
            "input":text()
        }),
        json!(["candidate_set_id", "revision", "request_id", "input"]),
    )
}

pub(super) fn refresh() -> Value {
    object_schema(
        json!({
            "candidate_set_id":uuid(),
            "revision":{"type":"integer","minimum":1},
            "request_id":uuid(),
            "program_revision":{"type":"integer","minimum":1}
        }),
        json!([
            "candidate_set_id",
            "revision",
            "request_id",
            "program_revision"
        ]),
    )
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
