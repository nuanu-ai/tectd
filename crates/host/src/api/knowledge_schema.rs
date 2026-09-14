use crate::tools::object_schema;
use serde_json::{Value, json};

use super::catalog::RouteSpec;

fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}

fn text(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}

fn digest() -> Value {
    json!({"type":"string","minLength":1,"maxLength":256})
}

fn binding() -> Value {
    json!({"oneOf":[
        object_schema(json!({"kind":{"const":"workspace"}}), json!(["kind"])),
        object_schema(
            json!({"kind":{"const":"slice_phase"},"scope_id":uuid(),"slice_id":uuid(),"phase_id":text(256)}),
            json!(["kind","scope_id","slice_id","phase_id"]),
        )
    ]})
}

fn draft() -> Value {
    let strings = json!({"type":"array","items":text(4096),"maxItems":64});
    object_schema(
        json!({
            "title":text(1024),"statement":text(65536),
            "modality":{"type":"string","enum":["must","must_not"]},
            "action":text(4096),"target_iri":{"type":"string","minLength":1,"maxLength":4096,
                "pattern":"^(https?://|urn:)"},
            "conditions":strings,"exceptions":{"type":"array","items":text(4096),"maxItems":64},
            "source":object_schema(
                json!({"title":text(1024),"uri":text(4096),"text":text(65536)}),
                json!(["title","uri","text"]),
            ),
            "binding":binding(),
            "purpose":{"const":"execution_constraint"},
            "version_resolution":{"const":"current_accepted"}
        }),
        json!([
            "title",
            "statement",
            "modality",
            "action",
            "target_iri",
            "conditions",
            "exceptions",
            "source",
            "binding",
            "purpose",
            "version_resolution"
        ]),
    )
}

pub(super) fn context() -> Value {
    json!({"oneOf":[
        object_schema(json!({}), json!([])),
        object_schema(json!({"unit_id":uuid()}), json!(["unit_id"])),
        object_schema(json!({"unit_id":uuid(),"revision":{"type":"integer","minimum":1}}),
            json!(["unit_id","revision"]))
    ]})
}

pub(super) fn change() -> Value {
    object_schema(json!({"change_id":uuid()}), json!(["change_id"]))
}

pub(super) fn prepare() -> Value {
    let proposal = draft();
    let generation = json!({"type":"integer","minimum":0});
    let revision = json!({"type":"integer","minimum":1});
    json!({"oneOf":[
        object_schema(
            json!({"request_id":uuid(),"operation":{"const":"create"},
                "expected_generation":generation,"draft":proposal,
                "reason":text(4096),"authority_basis":text(4096)}),
            json!(["request_id","operation","expected_generation","draft","reason","authority_basis"]),
        ),
        object_schema(
            json!({"request_id":uuid(),"operation":{"const":"revise"},"unit_id":uuid(),
                "expected_unit_revision":revision,"expected_generation":{"type":"integer","minimum":0},
                "draft":draft(),"reason":text(4096),"authority_basis":text(4096)}),
            json!(["request_id","operation","unit_id","expected_unit_revision",
                "expected_generation","draft","reason","authority_basis"]),
        ),
        object_schema(
            json!({"request_id":uuid(),"operation":{"const":"retract"},"unit_id":uuid(),
                "expected_unit_revision":{"type":"integer","minimum":1},
                "expected_generation":{"type":"integer","minimum":0},
                "reason":text(4096),"authority_basis":text(4096)}),
            json!(["request_id","operation","unit_id","expected_unit_revision",
                "expected_generation","reason","authority_basis"]),
        )
    ]})
}

fn method_read() -> Value {
    object_schema(
        json!({"id":text(256),"version":text(128),"digest":digest()}),
        json!(["id", "version", "digest"]),
    )
}

pub(super) fn review() -> Value {
    object_schema(
        json!({
            "request_id":uuid(),"change_id":uuid(),
            "change_revision":{"type":"integer","minimum":1},"proposal_digest":digest(),
            "verdict":{"type":"string","enum":["approve","reject"]},
            "review_summary":text(65536),"method_read":method_read()
        }),
        json!([
            "request_id",
            "change_id",
            "change_revision",
            "proposal_digest",
            "verdict",
            "review_summary",
            "method_read"
        ]),
    )
}

pub(super) fn publish() -> Value {
    object_schema(
        json!({"request_id":uuid(),"change_id":uuid(),
            "change_revision":{"type":"integer","minimum":1},"proposal_digest":digest()}),
        json!([
            "request_id",
            "change_id",
            "change_revision",
            "proposal_digest"
        ]),
    )
}

pub(super) fn refresh() -> Value {
    object_schema(
        json!({"request_id":uuid(),"run_id":uuid(),
            "run_revision":{"type":"integer","minimum":1},"phase_id":text(256)}),
        json!(["request_id", "run_id", "run_revision", "phase_id"]),
    )
}

macro_rules! route {
    ($tool:expr, $route:expr, $internal:expr, $summary:expr, $conditions:expr,
     $effects:expr, $retry:expr, $schema:expr, $example:expr $(,)?) => {
        RouteSpec {
            tool: $tool,
            route: $route,
            internal: $internal,
            summary: $summary,
            conditions: $conditions,
            effects: $effects,
            retry: $retry,
            schema: $schema,
            example: $example,
        }
    };
}

pub(super) fn routes(example_id: &str) -> Vec<RouteSpec> {
    vec![
        route!(
            "query",
            "knowledge.context",
            "knowledge_context",
            "Read current durable-knowledge capability, generation, methods, and an optional exact unit revision.",
            "Requires an authenticated open native session. revision is valid only with unit_id; omitted optional fields select the workspace context.",
            "Reads a consistent native RDF-backed snapshot without creating graphs or refreshing pipeline manifests.",
            "Safe to repeat. Use the returned generation and exact method digests in later commands.",
            context(),
            json!({}),
        ),
        route!(
            "query",
            "knowledge.change",
            "knowledge_change",
            "Read the current state of one durable Knowledge Change.",
            "Requires an authenticated open native session, current workspace membership, and an accessible change_id.",
            "Reads the proposal, method pins, review, and publication cursor without mutation.",
            "Safe to repeat. Follow the returned stage-specific action and exact revision and digest.",
            change(),
            json!({"change_id":example_id}),
        ),
        route!(
            "command",
            "knowledge.change_prepare",
            "knowledge_change_prepare",
            "Prepare one bounded create, revise, or retract Knowledge Change for exact review.",
            "Requires authenticated owner membership, the current workspace generation, operation-specific unit pins, source-derived proposal content for create or revise, and stated authority.",
            "Persists an immutable proposal and method pins only; it does not write canonical RDF or make knowledge eligible for delivery.",
            "The same request and byte-identical payload replays. Stale baselines must be re-read and are never auto-rebased.",
            prepare(),
            json!({"request_id":example_id,"operation":"create","expected_generation":0,
                "draft":{"title":"Bounded constraint","statement":"The phase must retain exact source provenance.",
                    "modality":"must","action":"retain","target_iri":"urn:tect:target:source-provenance",
                    "conditions":[],"exceptions":[],
                    "source":{"title":"Authoritative requirement","uri":"urn:source:requirement","text":"Retain exact source provenance."},
                    "binding":{"kind":"workspace"},"purpose":"execution_constraint",
                    "version_resolution":"current_accepted"},
                "reason":"Create the reviewed execution constraint.","authority_basis":"Current authenticated workspace owner instruction."}),
        ),
        route!(
            "command",
            "knowledge.change_review",
            "knowledge_change_review",
            "Record the exact semantic review of a pinned Knowledge Change proposal.",
            "Requires authenticated owner membership, current change revision and proposal digest, a substantive review summary, and the exact built-in review method receipt.",
            "Approves the exact proposal for publication or records a distinct rejection; it does not write canonical RDF.",
            "The same request and byte-identical payload replays. Reload the change before resolving any stale pin.",
            review(),
            json!({"request_id":example_id,"change_id":example_id,"change_revision":1,
                "proposal_digest":"0000000000000000000000000000000000000000000000000000000000000000",
                "verdict":"approve","review_summary":"Exact source, modality, conditions, exceptions, binding, and authority were reviewed.",
                "method_read":{"id":"tect:knowledge-change-review","version":"dk-1",
                    "digest":"0000000000000000000000000000000000000000000000000000000000000000"}}),
        ),
        route!(
            "command",
            "knowledge.change_publish",
            "knowledge_change_publish",
            "Atomically publish one approved exact Knowledge Change.",
            "Requires authenticated owner membership, ready_to_publish stage, exact change revision and proposal digest, and unchanged unit head and workspace generation.",
            "Atomically commits native RDF, SQL projections, publication event, receipt, generation, invalidation, and outbox state.",
            "The same request and byte-identical payload returns the original receipt and never republishes. Recover uncertain results by reading the change.",
            publish(),
            json!({"request_id":example_id,"change_id":example_id,"change_revision":2,
                "proposal_digest":"0000000000000000000000000000000000000000000000000000000000000000"}),
        ),
        route!(
            "command",
            "pipeline.knowledge_refresh",
            "pipeline_knowledge_refresh",
            "Explicitly refresh the immutable durable-knowledge manifest for the current pipeline phase.",
            "Requires an authenticated open native session and the exact active run revision and current phase.",
            "Captures a new native RDF-backed manifest and advances the run guard atomically without performing phase work.",
            "The same request and byte-identical payload replays. Reload pipeline context before resolving stale context.",
            refresh(),
            json!({"request_id":example_id,"run_id":example_id,"run_revision":2,
                "phase_id":"slice-lightweight-entry-gate"}),
        ),
    ]
}
