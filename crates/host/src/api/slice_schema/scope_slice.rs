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
    let matrix_selection = object_schema(
        json!({
            "task_id":uuid(),"task_revision":{"type":"integer","minimum":1},
            "disposition_id":uuid(),
            "selected_choice_id":{"type":"string","minLength":1,"maxLength":4096},
            "mapped_draft_node_indices":{"type":"array","items":{"type":"integer","minimum":0},"minItems":1,"maxItems":100,"uniqueItems":true,"description":"Nonempty, strictly increasing zero-based positions in the submitted draft.nodes array."},
            "expected_input_digest":matrix_digest(),
            "expected_choice_set_digest":matrix_digest(),
            "expected_verification_digest":matrix_digest()
        }),
        json!([
            "task_id",
            "task_revision",
            "disposition_id",
            "selected_choice_id",
            "mapped_draft_node_indices",
            "expected_input_digest",
            "expected_choice_set_digest",
            "expected_verification_digest"
        ]),
    );
    let envelope = |kind: &str, payload: (&str, Value)| {
        let mut properties = json!({
            "kind":{"const":kind},"scope_id":uuid(),"candidate_set_id":uuid(),
            "revision":{"type":"integer","minimum":1},"snapshot_id":uuid(),
            "input_cursor":{"type":"integer","minimum":0},"request_id":uuid(),
            "consumed_knowledge":super::planning_manifest_guard(),payload.0:payload.1
        });
        if kind == "draft" {
            properties["matrix_selection"] = matrix_selection.clone();
        }
        object_schema(
            properties,
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

pub(super) fn save_matrix_selection_example() -> Value {
    let mut example = save_example();
    let id = "00000000-0000-4000-8000-000000000001";
    let digest = "a".repeat(64);
    example["matrix_selection"] = json!({
        "task_id":id,"task_revision":1,"disposition_id":id,
        "selected_choice_id":"choice-a",
        "mapped_draft_node_indices":[0],
        "expected_input_digest":digest,
        "expected_choice_set_digest":digest,
        "expected_verification_digest":digest
    });
    example
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
fn matrix_digest() -> Value {
    json!({"type":"string","minLength":64,"maxLength":64,"pattern":"^[0-9a-f]{64}$"})
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
