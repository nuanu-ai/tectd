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
        page("overview"),page("program"),page("inputs"),page("candidates"),page("reviews"),page("history"),
        object_schema(
            json!({
                "candidate_set_id":uuid(),"view":{"const":"historical"},
                "draft_revision":{"type":"integer","minimum":2},
                "after":{"type":"integer","minimum":0},
                "limit":{"type":"integer","minimum":1,"maximum":100}
            }),
            json!(["candidate_set_id","view","draft_revision","limit"])
        ),
        object_schema(
            json!({
                "candidate_set_id":uuid(),"view":{"const":"fragment"},
                "draft_revision":{"type":"integer","minimum":2},
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
            "input":text(),
            "task_context":super::planning_task_context()
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

pub(super) fn delta_apply() -> Value {
    let op = |name: &str, properties: Value, required: Value| {
        let mut properties = properties.as_object().cloned().unwrap();
        properties.insert("operation".into(), json!({"const":name}));
        let mut required = required.as_array().cloned().unwrap();
        required.insert(0, json!("operation"));
        object_schema(Value::Object(properties), Value::Array(required))
    };
    let source_value = || json!({"summary":text(),"source_ref_id":uuid()});
    let mut evidence_update = source_value();
    evidence_update["evidence_id"] = uuid();
    evidence_update["expected_revision"] = json!({"type":"integer","minimum":1});
    let mut blocker_update = source_value();
    blocker_update["blocker_id"] = uuid();
    blocker_update["expected_revision"] = json!({"type":"integer","minimum":1});
    let operations = json!({"oneOf":[
        op("goal.add",json!({"goal_id":uuid(),"text":text(),"finite":{"type":"boolean"},"source_ref_id":uuid()}),json!(["goal_id","text","finite","source_ref_id"])),
        op("goal.resolve",json!({"goal_id":uuid(),"expected_revision":{"type":"integer","minimum":1}}),json!(["goal_id","expected_revision"])),
        op("candidate.add",json!({"candidate_id":uuid(),"title":text(),"outcome":{"type":"string"}}),json!(["candidate_id","title"])),
        op("candidate.update",json!({"candidate_id":uuid(),"expected_revision":{"type":"integer","minimum":1},"title":text(),"outcome":{"type":"string"}}),json!(["candidate_id","expected_revision","title"])),
        op("candidate.remove",json!({"candidate_id":uuid(),"expected_revision":{"type":"integer","minimum":1}}),json!(["candidate_id","expected_revision"])),
        op("candidate.supersede",json!({"candidate_id":uuid(),"replacement_candidate_id":uuid(),"expected_revision":{"type":"integer","minimum":1}}),json!(["candidate_id","replacement_candidate_id","expected_revision"])),
        op("coverage.link",json!({"candidate_id":uuid(),"goal_id":uuid()}),json!(["candidate_id","goal_id"])),
        op("coverage.unlink",json!({"candidate_id":uuid(),"goal_id":uuid()}),json!(["candidate_id","goal_id"])),
        op("evidence.add",json!({"evidence_id":uuid(),"target_kind":{"type":"string","enum":["goal","candidate"]},"target_id":uuid(),"summary":text(),"source_ref_id":uuid()}),json!(["evidence_id","target_kind","target_id","summary","source_ref_id"])),
        op("evidence.update",evidence_update,json!(["evidence_id","expected_revision","summary","source_ref_id"])),
        op("evidence.remove",json!({"evidence_id":uuid(),"expected_revision":{"type":"integer","minimum":1}}),json!(["evidence_id","expected_revision"])),
        op("blocker.add",json!({"blocker_id":uuid(),"goal_id":uuid(),"summary":text(),"source_ref_id":uuid()}),json!(["blocker_id","goal_id","summary","source_ref_id"])),
        op("blocker.update",blocker_update,json!(["blocker_id","expected_revision","summary","source_ref_id"])),
        op("blocker.remove",json!({"blocker_id":uuid(),"expected_revision":{"type":"integer","minimum":1}}),json!(["blocker_id","expected_revision"]))
    ]});
    object_schema(
        json!({
            "candidate_set_id":uuid(),
            "expected_revision":{"type":"integer","minimum":1},
            "idempotency_key":{"type":"string","minLength":1,"maxLength":128},
            "operations":{"type":"array","minItems":1,"maxItems":100,"items":operations}
        }),
        json!([
            "candidate_set_id",
            "expected_revision",
            "idempotency_key",
            "operations"
        ]),
    )
}

pub(super) fn delta_status() -> Value {
    object_schema(
        json!({"candidate_set_id":uuid(),"idempotency_key":{"type":"string","minLength":1,"maxLength":128}}),
        json!(["candidate_set_id", "idempotency_key"]),
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
            "evidence":{"type":"array","items":reference,"maxItems":100},
            "change_rationale":text()
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
    let supersession = object_schema(
        json!({
            "candidate_id":uuid(),
            "revision":{"type":"integer","minimum":1},
            "reason":text(),
            "replacements":{"type":"array","items":reference,"maxItems":100}
        }),
        json!(["candidate_id", "revision", "reason"]),
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
            "protected_changes":{"type":"array","items":protected_change,"maxItems":100},
            "supersessions":{"type":"array","items":supersession,"maxItems":100}
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
                    "consumed_knowledge":super::planning_manifest_guard(),
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
                    "consumed_knowledge":super::planning_manifest_guard(),
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
            "program_revision":{"type":"integer","minimum":1},
            "task_context":super::planning_task_context()
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
