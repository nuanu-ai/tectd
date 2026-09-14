use super::catalog::RouteSpec;
use crate::tools::object_schema;
use serde_json::{Value, json};

mod knowledge_document_schema;
mod knowledge_profile_schema;
mod phase_schema;

fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}
fn text(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn digest() -> Value {
    text(256)
}
fn computed_digest() -> Value {
    json!({"type":"string","maxLength":256})
}
fn revision() -> Value {
    json!({"type":"integer","minimum":1})
}
fn generation() -> Value {
    json!({"type":"integer","minimum":0})
}
fn strings(max: usize) -> Value {
    strings_with_limit(max, 128)
}
fn strings_with_limit(max: usize, max_items: usize) -> Value {
    json!({"type":"array","items":text(max),"maxItems":max_items,"uniqueItems":true})
}
fn uuids() -> Value {
    json!({"type":"array","items":uuid(),"maxItems":128,"uniqueItems":true})
}
fn phases() -> Value {
    json!({"type":"string","enum":["kc-intake","kc-resolve-baseline","kc-qualify-plan","kc-qualify-evidence","kc-prepare-change","kc-domain-checks","kc-impact-plan","kc-review-reconcile","kc-publication-gate","kc-commit","kc-settle-effects","kc-result-handoff"]})
}

pub(super) fn routes(example: &str) -> Vec<RouteSpec> {
    vec![
        spec(
            "query",
            "knowledge.lifecycle",
            "knowledge_lifecycle",
            "Read an active Knowledge Change, its immutable attempt history, or one exact output.",
            "change_id is omitted only for the workspace active overview. output view requires change_id, output_id and digest.",
            "Read-only exact lifecycle state and receipts.",
            "Safe to repeat with the exact identifiers and digest.",
            lifecycle(),
            json!({"change_id":example,"view":"current"}),
        ),
        spec(
            "query",
            "knowledge.unit",
            "knowledge_unit",
            "Read one exact canonical knowledge unit revision, including typed document and RDF identity receipts.",
            "Requires a unit_id; omitted revision resolves the current visible head. Erased payload returns only its tombstone.",
            "Read-only canonical legacy Constraint or typed DK-2 document projection.",
            "Safe to repeat; pin revision for an exact historical read.",
            unit(),
            json!({"unit_id":example,"revision":1}),
        ),
        spec(
            "command",
            "knowledge.change_begin",
            "knowledge_change_begin",
            "Begin or replay one persisted 12-phase Knowledge Change run.",
            "Requires 1 to 16 operation hints, exact saved source references, owner, completion contract and an authenticated workspace owner. Erase requires an explicit supported erasure scope; all_retained_copies is unavailable in DK-2 and is refused before commit.",
            "Creates backend operation/unit identities and a definition-pinned run; it publishes nothing.",
            "The same request_id and byte-identical request replays the same run.",
            begin(),
            json!({"request_id":example,"intent":"Correct one bounded knowledge unit.","desired_outcome":"Publish an exact reviewed current result.","sources":[],"operation_hints":[{"client_label":"change-1","operation":"create","reason":"New supported knowledge.","authority_basis":"Current workspace owner."}],"owner":{"kind":"workspace"},"completion":{"canonical_result":true,"exact_delivery":true,"impact_recorded":true,"search":"not_required","erasure":"not_required"}}),
        ),
        spec(
            "command",
            "knowledge.change_phase_complete",
            "knowledge_change_phase_complete",
            "Record an exact phase attempt or invoke the current backend machine phase.",
            "Agent phases require the phase-tagged substantive output and exact pins/receipts. KC-09 and KC-11 are backend phases and accept no invented semantic output.",
            "Appends immutable attempt/output state, validates dependencies and advances or revisits the pinned run.",
            "Exact request replay returns the original logical result; use returned run_revision for new work.",
            phase_complete(),
            json!({"request_id":example,"change_id":example,"run_id":example,"run_revision":1,"phase_id":"kc-publication-gate"}),
        ),
        spec(
            "command",
            "knowledge.change_record_input",
            "knowledge_change_record_input",
            "Record explicit new context and revisit a pre-commit Knowledge Change phase.",
            "Requires current run revision, a permitted KC-02 through KC-07 revisit phase, nonblank reason and exact input.",
            "Persists input history and invalidates dependent outputs before moving the cursor backward.",
            "The same request and payload replays; changed payload conflicts.",
            record_input(),
            json!({"request_id":example,"change_id":example,"run_id":example,"run_revision":4,"revisit_phase_id":"kc-qualify-evidence","reason":"New source changes the evidence basis.","input":"Exact additional context."}),
        ),
        spec(
            "command",
            "knowledge.change_commit",
            "knowledge_change_commit",
            "Apply the exact KC-09 sealed compound command atomically.",
            "Requires backend-issued seal, plan and command digests plus current run revision and current owner authorization.",
            "Performs the sole canonical RDF apply boundary and records publisher/effect receipts atomically.",
            "Exact replay returns the historical receipt after current authorization succeeds.",
            commit(),
            json!({"request_id":example,"change_id":example,"run_id":example,"run_revision":9,"seal_id":example,"plan_revision":1,"plan_digest":"sha256","sealed_command_digest":"sha256"}),
        ),
        spec(
            "command",
            "knowledge.change_settle_effects",
            "knowledge_change_settle_effects",
            "Settle selected backend-issued post-commit effects without republishing.",
            "Requires the exact publisher receipt, current run revision and only effect IDs returned by the backend.",
            "Idempotently accounts for delivery, impact, search, visibility and owned-copy effects.",
            "Exact replay returns the original report; unresolved effects remain explicit.",
            settle(),
            json!({"request_id":example,"change_id":example,"run_id":example,"run_revision":11,"publisher_receipt_id":example,"effect_ids":[]}),
        ),
    ]
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

fn lifecycle() -> Value {
    let fragment = object_schema(
        json!({"snapshot_digest":digest(),"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":262144}}),
        json!(["offset", "limit"]),
    );
    json!({"oneOf":[
        object_schema(json!({"fragment":fragment.clone()}),json!([])),
        object_schema(json!({"change_id":uuid(),"view":{"const":"current"},"fragment":fragment.clone()}),json!(["change_id","view"])),
        object_schema(json!({"change_id":uuid(),"view":{"const":"history"},"fragment":fragment.clone()}),json!(["change_id","view"])),
        object_schema(json!({"change_id":uuid(),"view":{"const":"output"},"output_id":uuid(),"digest":digest(),"fragment":fragment}),json!(["change_id","view","output_id","digest"]))
    ]})
}
fn unit() -> Value {
    let fragment = object_schema(
        json!({"snapshot_digest":digest(),"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":262144}}),
        json!(["offset", "limit"]),
    );
    json!({"oneOf":[object_schema(json!({"unit_id":uuid(),"fragment":fragment.clone()}),json!(["unit_id"])),object_schema(json!({"unit_id":uuid(),"revision":revision(),"fragment":fragment}),json!(["unit_id","revision"]))]})
}

fn operation_hint() -> Value {
    json!({"oneOf":[
        object_schema(json!({"client_label":text(128),"operation":{"const":"create"},"reason":text(4096),"authority_basis":text(4096),"depends_on_labels":strings(128)}),json!(["client_label","operation","reason","authority_basis"])),
        object_schema(json!({"client_label":text(128),"operation":{"enum":["revise","revalidate","supersede","retract","erase"]},"unit_id":uuid(),"expected_revision":revision(),"expected_lifecycle":{"enum":["active","retracted","superseded","erasure_pending","erased"]},"reason":text(4096),"authority_basis":text(4096),"depends_on_labels":strings(128)}),json!(["client_label","operation","unit_id","expected_revision","expected_lifecycle","reason","authority_basis"]))
    ]})
}
fn begin() -> Value {
    object_schema(
        json!({
            "request_id":uuid(),"intent":text(262144),"desired_outcome":text(4096),"sources":{"type":"array","items":knowledge_document_schema::source(),"maxItems":128},
            "operation_hints":{"type":"array","items":operation_hint(),"minItems":1,"maxItems":16,"uniqueItems":true},
            "owner":json!({"oneOf":[object_schema(json!({"kind":{"const":"workspace"}}),json!(["kind"])),object_schema(json!({"kind":{"const":"promotion_slice"},"scope_id":uuid(),"slice_id":uuid(),"slice_revision":revision()}),json!(["kind","scope_id","slice_id","slice_revision"]))]}),
            "completion":object_schema(json!({"canonical_result":{"type":"boolean"},"exact_delivery":{"type":"boolean"},"impact_recorded":{"type":"boolean"},"search":{"enum":["not_required","required"]},"erasure":{"enum":["not_required","owned_live_copies","restore_safe","all_retained_copies"]}}),json!(["canonical_result","exact_delivery","impact_recorded","search","erasure"])),
            "delivery_mode":{"enum":["whole","phasewise"]}
        }),
        json!([
            "request_id",
            "intent",
            "desired_outcome",
            "sources",
            "operation_hints",
            "owner",
            "completion"
        ]),
    )
}

fn consumed_output() -> Value {
    object_schema(
        json!({"phase_id":text(256),"output_revision":revision(),"digest":digest()}),
        json!(["phase_id", "output_revision", "digest"]),
    )
}
fn consumed_input() -> Value {
    object_schema(
        json!({"input_id":uuid(),"sequence":revision(),"digest":digest()}),
        json!(["input_id", "sequence", "digest"]),
    )
}
fn method_read() -> Value {
    object_schema(
        json!({"instruction_id":text(256),"version":text(128),"digest":digest()}),
        json!(["instruction_id", "version", "digest"]),
    )
}
fn guard() -> Value {
    object_schema(
        json!({"unit_id":uuid(),"revision":revision(),"lifecycle":{"enum":["active","retracted","superseded","erasure_pending","erased"]},"rdf_digest":digest(),"unit_iri":text(4096),"revision_iri":text(4096)}),
        json!([
            "unit_id",
            "revision",
            "lifecycle",
            "rdf_digest",
            "unit_iri",
            "revision_iri"
        ]),
    )
}
fn output() -> Value {
    object_schema(
        json!({"phase_id":phases(),"expected_run_revision":revision(),"phasewise_reason":text(4096),"plan_revision":generation(),"plan_digest":computed_digest(),"consumed_outputs":{"type":"array","items":consumed_output(),"maxItems":128},"consumed_inputs":{"type":"array","items":consumed_input(),"maxItems":128},"baseline_guards":{"type":"array","items":guard(),"maxItems":32},"source_digests":strings(256),"method_reads":{"type":"array","items":method_read(),"maxItems":128},"body":text(262144),"data":phase_schema::phase_data(),"verdict":text(256),"outcome":{"enum":["completed","waiting_input","blocked"]},"transition":{"enum":["continue","complete","block","escalate"]},"findings":{"type":"array","items":phase_schema::review_finding(),"maxItems":128},"dispositions":strings(4096)}),
        json!([
            "phase_id",
            "expected_run_revision",
            "plan_revision",
            "plan_digest",
            "consumed_outputs",
            "consumed_inputs",
            "baseline_guards",
            "source_digests",
            "method_reads",
            "body",
            "data",
            "verdict",
            "outcome",
            "transition",
            "findings",
            "dispositions"
        ]),
    )
}
fn phase_complete() -> Value {
    json!({"oneOf":[object_schema(json!({"request_id":uuid(),"change_id":uuid(),"run_id":uuid(),"run_revision":revision(),"phase_id":{"enum":["kc-publication-gate","kc-settle-effects"]},"revisit_phase_id":phases()}),json!(["request_id","change_id","run_id","run_revision","phase_id"])),object_schema(json!({"request_id":uuid(),"change_id":uuid(),"run_id":uuid(),"run_revision":revision(),"phase_id":{"enum":["kc-intake","kc-resolve-baseline","kc-qualify-plan","kc-qualify-evidence","kc-prepare-change","kc-domain-checks","kc-impact-plan","kc-review-reconcile","kc-result-handoff"]},"output":output(),"revisit_phase_id":phases()}),json!(["request_id","change_id","run_id","run_revision","phase_id","output"]))]})
}
fn target_basis_update() -> Value {
    object_schema(
        json!({"operation_id":uuid(),"previous_expected_revision":revision(),
            "previous_expected_lifecycle":{"enum":["active","retracted","superseded","erasure_pending","erased"]},
            "replacement_guard":guard()}),
        json!([
            "operation_id",
            "previous_expected_revision",
            "previous_expected_lifecycle",
            "replacement_guard"
        ]),
    )
}
fn basis_amendment() -> Value {
    let targets = |minimum: usize, maximum: usize| {
        json!({"type":"array","items":target_basis_update(),"minItems":minimum,
            "maxItems":maximum,"uniqueItems":true})
    };
    let sources =
        json!({"type":"array","items":knowledge_document_schema::source(),"maxItems":128});
    json!({"oneOf":[
        object_schema(json!({"target_updates":targets(1,16),"replacement_sources":sources.clone()}),json!(["target_updates"])),
        object_schema(json!({"target_updates":targets(0,0),"replacement_sources":sources}),json!(["target_updates","replacement_sources"]))
    ]})
}
fn record_input() -> Value {
    object_schema(
        json!({"request_id":uuid(),"change_id":uuid(),"run_id":uuid(),"run_revision":revision(),"revisit_phase_id":{"enum":["kc-resolve-baseline","kc-qualify-plan","kc-qualify-evidence","kc-prepare-change","kc-domain-checks","kc-impact-plan"]},"reason":text(4096),"input":text(262144),"basis_amendment":basis_amendment()}),
        json!([
            "request_id",
            "change_id",
            "run_id",
            "run_revision",
            "revisit_phase_id",
            "reason",
            "input"
        ]),
    )
}
fn commit() -> Value {
    object_schema(
        json!({"request_id":uuid(),"change_id":uuid(),"run_id":uuid(),"run_revision":revision(),"seal_id":uuid(),"plan_revision":revision(),"plan_digest":digest(),"sealed_command_digest":digest()}),
        json!([
            "request_id",
            "change_id",
            "run_id",
            "run_revision",
            "seal_id",
            "plan_revision",
            "plan_digest",
            "sealed_command_digest"
        ]),
    )
}
fn settle() -> Value {
    object_schema(
        json!({"request_id":uuid(),"change_id":uuid(),"run_id":uuid(),"run_revision":revision(),"publisher_receipt_id":uuid(),"effect_ids":uuids()}),
        json!([
            "request_id",
            "change_id",
            "run_id",
            "run_revision",
            "publisher_receipt_id"
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposes_exact_routes_and_phase_tagged_output_contracts() {
        let routes = routes("00000000-0000-4000-8000-000000000001");
        assert_eq!(routes.len(), 7);
        assert_eq!(routes.iter().filter(|r| r.tool == "query").count(), 2);
        assert_eq!(routes.iter().filter(|r| r.tool == "command").count(), 5);
        let phase = routes
            .iter()
            .find(|r| r.route == "knowledge.change_phase_complete")
            .unwrap();
        let rendered = serde_json::to_string(&phase.schema).unwrap();
        for id in [
            "kc-intake",
            "kc-resolve-baseline",
            "kc-qualify-plan",
            "kc-qualify-evidence",
            "kc-prepare-change",
            "kc-domain-checks",
            "kc-impact-plan",
            "kc-review-reconcile",
            "kc-publication-gate",
            "kc-settle-effects",
            "kc-result-handoff",
        ] {
            assert!(rendered.contains(id));
        }
        assert!(rendered.contains("method_reads"));
        assert!(rendered.contains("consumed_outputs"));
        assert!(rendered.contains("output_revision"));
        assert!(
            phase.schema["oneOf"][1]["properties"]["output"]["properties"]
                ["consumed_outputs"]["items"]["properties"]
                .get("output_digest")
                .is_none()
        );
        assert!(rendered.contains("publisher_receipt_id"));
        let lifecycle = routes
            .iter()
            .find(|r| r.route == "knowledge.lifecycle")
            .unwrap();
        let rendered = serde_json::to_string(&lifecycle.schema).unwrap();
        assert!(rendered.contains("snapshot_digest"));
        assert!(rendered.contains("262144"));
        let unit = routes.iter().find(|r| r.route == "knowledge.unit").unwrap();
        let rendered = serde_json::to_string(&unit.schema).unwrap();
        assert!(rendered.contains("snapshot_digest"));
        assert!(rendered.contains("262144"));
    }

    #[test]
    fn begin_schema_exposes_scoped_erasure_and_six_operations() {
        let rendered = serde_json::to_string(&begin()).unwrap();
        for operation in [
            "create",
            "revise",
            "revalidate",
            "supersede",
            "retract",
            "erase",
        ] {
            assert!(rendered.contains(operation));
        }
        for scope in [
            "not_required",
            "owned_live_copies",
            "restore_safe",
            "all_retained_copies",
        ] {
            assert!(rendered.contains(scope));
        }
        assert!(!rendered.contains("complete_erasure"));
    }

    #[test]
    fn phase_and_input_schemas_expose_zero_pins_and_typed_basis_reconciliation() {
        let output = output();
        assert_eq!(output["properties"]["plan_revision"]["minimum"], 0);
        assert!(
            output["properties"]["plan_digest"]
                .get("minLength")
                .is_none()
        );
        assert_eq!(output["properties"]["phasewise_reason"]["maxLength"], 4096);
        assert_eq!(output["properties"]["consumed_inputs"]["maxItems"], 128);
        assert_eq!(output["properties"]["method_reads"]["maxItems"], 128);
        let method = &output["properties"]["method_reads"]["items"];
        assert!(method["properties"].get("instruction_id").is_some());
        assert!(method["properties"].get("id").is_none());
        let rendered = serde_json::to_string(&output).unwrap();
        for field in ["assessment_conflicts", "assessment_gaps"] {
            assert!(rendered.contains(field));
        }
        let variants = output["properties"]["data"]["oneOf"].as_array().unwrap();
        let domain_checks = variants
            .iter()
            .find(|variant| variant["properties"]["phase"]["const"] == "kc-domain-checks")
            .unwrap();
        assert_eq!(
            domain_checks["properties"]["data"]["properties"]["unresolved_obligation_ids"]["maxItems"],
            tect_domain::DK2_MAX_OPERATIONS * 64
        );
        let review = variants
            .iter()
            .find(|variant| variant["properties"]["phase"]["const"] == "kc-review-reconcile")
            .unwrap();
        assert_eq!(
            review["properties"]["data"]["properties"]["covered_obligation_ids"]["maxItems"],
            tect_domain::DK2_MAX_OPERATIONS * 64
        );
        let prepare = variants
            .iter()
            .find(|variant| variant["properties"]["phase"]["const"] == "kc-prepare-change")
            .unwrap();
        let operations =
            &prepare["properties"]["data"]["properties"]["operations"]["items"]["oneOf"];
        assert_eq!(operations.as_array().unwrap().len(), 5);
        assert_eq!(operations[0]["properties"]["binding_pins"]["maxItems"], 0);
        assert!(
            !operations[0]["required"]
                .as_array()
                .unwrap()
                .contains(&json!("binding_pins"))
        );
        assert_eq!(
            operations[0]["properties"]["document"]["properties"]["sections"]["additionalProperties"],
            false
        );
        for (phase, field) in [
            ("kc-qualify-evidence", "claims"),
            ("kc-domain-checks", "receipts"),
            ("kc-impact-plan", "affected_contexts"),
            ("kc-review-reconcile", "findings"),
        ] {
            let variant = variants
                .iter()
                .find(|item| item["properties"]["phase"]["const"] == phase)
                .unwrap();
            assert_eq!(
                variant["properties"]["data"]["properties"][field]["items"]["additionalProperties"],
                false
            );
        }
        let record = record_input();
        let amendment = &record["properties"]["basis_amendment"];
        assert!(amendment["oneOf"].is_array());
        let rendered = serde_json::to_string(amendment).unwrap();
        for field in [
            "target_updates",
            "previous_expected_revision",
            "previous_expected_lifecycle",
            "replacement_guard",
            "replacement_sources",
        ] {
            assert!(rendered.contains(field));
        }
    }
}
