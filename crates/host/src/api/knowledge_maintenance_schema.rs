use super::catalog::RouteSpec;
use crate::tools::object_schema;
use serde_json::{Value, json};

fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}

fn text(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}

fn digest() -> Value {
    text(256)
}

fn fragment() -> Value {
    object_schema(
        json!({
            "snapshot_digest":digest(),
            "offset":{"type":"integer","minimum":0},
            "limit":{"type":"integer","minimum":1,"maximum":262144}
        }),
        json!(["offset", "limit"]),
    )
}

fn query() -> Value {
    object_schema(
        json!({
            "unit_id":uuid(),
            "states":{"type":"array","items":{"type":"string","enum":["pending","leased","needs_review","linked","resolved","obsolete","exhausted"]},"maxItems":7,"uniqueItems":true},
            "after":uuid(),
            "limit":{"type":"integer","minimum":1,"maximum":100,"default":25},
            "fragment":fragment()
        }),
        json!(["limit"]),
    )
}

fn external_basis() -> Value {
    json!({"oneOf":[
        object_schema(
            json!({"kind":{"const":"source_changed"},"source_iri":text(4096),"accepted_digest":digest(),"observed_digest":digest()}),
            json!(["kind","source_iri","accepted_digest","observed_digest"]),
        ),
        object_schema(
            json!({"kind":{"const":"application_failed"},"consumer_ref":text(4096),"failure_digest":digest()}),
            json!(["kind","consumer_ref","failure_digest"]),
        ),
        object_schema(
            json!({"kind":{"const":"operator_requested"},"subject_ref":text(4096),"observation_digest":digest()}),
            json!(["kind","subject_ref","observation_digest"]),
        )
    ]})
}

fn observe() -> Value {
    object_schema(
        json!({
            "request_id":uuid(),
            "unit_id":uuid(),
            "unit_revision":{"type":"integer","minimum":1},
            "basis":external_basis()
        }),
        json!(["request_id", "unit_id", "unit_revision", "basis"]),
    )
}

fn maintenance_hint() -> Value {
    object_schema(
        json!({
            "client_label":text(128),
            "operation":{"enum":["revalidate","revise","supersede"]},
            "unit_id":uuid(),
            "expected_revision":{"type":"integer","minimum":1},
            "expected_lifecycle":{"enum":["active","retracted","superseded","erasure_pending","erased"]},
            "reason":text(4096),
            "authority_basis":text(4096),
            "depends_on_labels":{"type":"array","items":text(128),"maxItems":128,"uniqueItems":true}
        }),
        json!([
            "client_label",
            "operation",
            "unit_id",
            "expected_revision",
            "expected_lifecycle",
            "reason",
            "authority_basis"
        ]),
    )
}

fn begin() -> Value {
    let maintenance_change = json!({"allOf":[
        super::knowledge_lifecycle_schema::begin(),
        {"type":"object","properties":{
            "owner":object_schema(json!({"kind":{"const":"workspace"}}),json!(["kind"])),
            "operation_hints":{"type":"array","items":maintenance_hint(),"minItems":1,"maxItems":1,"uniqueItems":true}
        }}
    ]});
    object_schema(
        json!({
            "request_id":uuid(),
            "task_id":uuid(),
            "task_revision":{"type":"integer","minimum":1},
            "change":maintenance_change
        }),
        json!(["request_id", "task_id", "task_revision", "change"]),
    )
}

#[allow(clippy::too_many_arguments)]
fn spec(
    tool: &'static str,
    route: &'static str,
    internal: &'static str,
    summary: &'static str,
    conditions: &'static str,
    effects: &'static str,
    retry: &'static str,
    schema: Value,
    example: Value,
) -> RouteSpec {
    RouteSpec {
        tool,
        route,
        internal,
        summary,
        conditions,
        effects,
        retry,
        schema,
        example,
    }
}

pub(super) fn routes(example: &str) -> Vec<RouteSpec> {
    vec![
        spec(
            "query",
            "knowledge.maintenance",
            "knowledge_maintenance",
            "Read an owner-only bounded page of durable knowledge maintenance tasks.",
            "Requires an authenticated workspace owner. Omitted filters read all task states; optional fragments use exact UTF-8 snapshot pins.",
            "Reads maintenance tasks, affected consumers, linked Knowledge Changes, and the exact maintenance method without mutation.",
            "Safe to repeat. Follow next_after or the returned fragment action unchanged.",
            query(),
            json!({"states":["needs_review"],"limit":25}),
        ),
        spec(
            "command",
            "knowledge.maintenance_observe",
            "knowledge_maintenance_observe",
            "Record one authenticated external maintenance signal for an exact knowledge revision.",
            "Requires a workspace owner and one SourceChanged, ApplicationFailed, or OperatorRequested basis. Backend-only review and dependency signals are rejected.",
            "Creates, replays, or returns the existing deduplicated maintenance task; it does not crawl, approve, or publish knowledge.",
            "Repeat only with the same request_id and byte-identical observation.",
            observe(),
            json!({"request_id":example,"unit_id":example,"unit_revision":1,"basis":{"kind":"operator_requested","subject_ref":"urn:operator:review","observation_digest":"sha256"}}),
        ),
        spec(
            "command",
            "knowledge.maintenance_begin",
            "knowledge_maintenance_begin",
            "Link one exact maintenance task revision to a workspace-owned Knowledge Change.",
            "Requires a needs-review task and one exact revalidate, revise, or supersede operation. Semantic intent, sources, authority, and completion requirements remain explicit caller inputs.",
            "Atomically creates or replays the linked 12-phase Knowledge Change and records the task linkage; it publishes nothing.",
            "Repeat only with the same request_id and byte-identical request; reload the task after a stale revision.",
            begin(),
            json!({
                "request_id":example,"task_id":example,"task_revision":1,
                "change":{"request_id":example,"intent":"Review the observed source change.","desired_outcome":"Publish the reviewed current result.","sources":[],"operation_hints":[{"client_label":"maintenance-1","operation":"revalidate","unit_id":example,"expected_revision":1,"expected_lifecycle":"active","reason":"Observed basis requires review.","authority_basis":"Current workspace owner."}],"owner":{"kind":"workspace"},"completion":{"canonical_result":true,"exact_delivery":true,"impact_recorded":true,"search":"not_required","erasure":"not_required"}}
            }),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::Error;

    #[test]
    fn schemas_and_runtime_parser_reject_backend_signals_nulls_and_unknowns() {
        for route in routes("00000000-0000-4000-8000-000000000001") {
            assert!(
                crate::api::decode_public_call(
                    route.tool,
                    json!({"route":route.route,"params":route.example})
                )
                .is_ok()
            );
        }
        for invalid in [
            json!({"request_id":"00000000-0000-4000-8000-000000000001","unit_id":"00000000-0000-4000-8000-000000000001","unit_revision":1,"basis":{"kind":"review_due","review_due_at":"2026-09-15T00:00:00Z"}}),
            json!({"request_id":"00000000-0000-4000-8000-000000000001","unit_id":"00000000-0000-4000-8000-000000000001","unit_revision":1,"basis":null}),
            json!({"request_id":"00000000-0000-4000-8000-000000000001","unit_id":"00000000-0000-4000-8000-000000000001","unit_revision":1,"basis":{"kind":"operator_requested","subject_ref":"urn:x","observation_digest":"d"},"approve":true}),
        ] {
            assert!(matches!(
                crate::knowledge_maintenance_tools::parse("knowledge_maintenance_observe", invalid),
                Err(Error::InvalidArguments | Error::InvalidArgumentsDetail(_))
            ));
        }
    }
}
