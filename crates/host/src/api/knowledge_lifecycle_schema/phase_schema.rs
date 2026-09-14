use crate::tools::object_schema;
use serde_json::{Value, json};

use super::{computed_digest, digest, generation, revision, text, uuid};

const LIST: usize = tect_domain::DK2_MAX_LIST_ITEMS;
const OPERATIONS: usize = tect_domain::DK2_MAX_OPERATIONS;
const OBLIGATIONS: usize = tect_domain::DK2_MAX_OPERATIONS * 64;

fn array(items: Value, max: usize) -> Value {
    json!({"type":"array","items":items,"maxItems":max})
}
fn strings(bytes: usize, max: usize) -> Value {
    json!({"type":"array","items":text(bytes),"maxItems":max})
}
fn bounded(bytes: usize) -> Value {
    json!({"type":"string","maxLength":bytes})
}
fn unique_uuids(max: usize) -> Value {
    json!({"type":"array","items":uuid(),"maxItems":max,"uniqueItems":true})
}
fn lifecycle() -> Value {
    json!({"enum":["active","retracted","superseded","erasure_pending","erased"]})
}
fn profile() -> Value {
    json!({"enum":["general","runbook","protocol","devops","operations","product_research","security"]})
}
fn knowledge_kind() -> Value {
    json!({"enum":["constraint","claim","decision","hypothesis","procedure","protocol","infrastructure","operating_model","product_research","security"]})
}
fn evidence_kind() -> Value {
    json!({"enum":["document","declaration","observation","decision_record","research","static_verification","runtime_verification","negative_evidence"]})
}
fn phase_id() -> Value {
    json!({"enum":["kc-intake","kc-resolve-baseline","kc-qualify-plan","kc-qualify-evidence","kc-prepare-change","kc-domain-checks","kc-impact-plan","kc-review-reconcile","kc-publication-gate","kc-commit","kc-settle-effects","kc-result-handoff"]})
}
fn completion() -> Value {
    object_schema(
        json!({"canonical_result":{"type":"boolean"},"exact_delivery":{"type":"boolean"},"impact_recorded":{"type":"boolean"},"search":{"enum":["not_required","required"]},"erasure":{"enum":["not_required","owned_live_copies","restore_safe","all_retained_copies"]}}),
        json!([
            "canonical_result",
            "exact_delivery",
            "impact_recorded",
            "search",
            "erasure"
        ]),
    )
}
fn guard() -> Value {
    object_schema(
        json!({"unit_id":uuid(),"revision":revision(),"lifecycle":lifecycle(),"rdf_digest":digest(),"unit_iri":text(4096),"revision_iri":text(4096)}),
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
fn identity_match() -> Value {
    object_schema(
        json!({"client_label":text(128),"unit_id":uuid(),"revision":revision(),"basis":text(4096),"ambiguous":{"type":"boolean"}}),
        json!(["client_label", "unit_id", "revision", "basis", "ambiguous"]),
    )
}
fn source_pin() -> Value {
    object_schema(
        json!({"source_index":{"type":"integer","minimum":0,"maximum":127},"digest":digest(),"evidence_kind":evidence_kind(),"observed_at":{"type":"string","format":"date-time","maxLength":128},"evidence_scope":text(4096),"source_iri":text(4096)}),
        json!([
            "source_index",
            "digest",
            "evidence_kind",
            "evidence_scope",
            "source_iri"
        ]),
    )
}
fn evidence_claim() -> Value {
    object_schema(
        json!({"operation_id":uuid(),"claim":text(262144),"source_indexes":{"type":"array","items":{"type":"integer","minimum":0,"maximum":127},"maxItems":LIST,"uniqueItems":true},"assumptions":strings(4096,LIST),"gaps":strings(4096,LIST)}),
        json!([
            "operation_id",
            "claim",
            "source_indexes",
            "assumptions",
            "gaps"
        ]),
    )
}
fn qualification() -> Value {
    object_schema(
        json!({"operation_id":uuid(),"knowledge_kind":knowledge_kind(),"profiles":{"type":"array","items":profile(),"minItems":1,"maxItems":7,"uniqueItems":true},"classification_basis":text(4096)}),
        json!([
            "operation_id",
            "knowledge_kind",
            "profiles",
            "classification_basis"
        ]),
    )
}
fn successor() -> Value {
    json!({"oneOf":[
        object_schema(json!({"unit_id":uuid()}),json!(["unit_id"])),
        object_schema(json!({"operation_id":uuid()}),json!(["operation_id"]))
    ]})
}
fn revalidation() -> Value {
    object_schema(
        json!({"sources":{"type":"array","items":super::knowledge_document_schema::source(),"minItems":1,"maxItems":LIST},"evidence_basis":text(262144),"valid_until":{"type":"string","format":"date-time","maxLength":128},"review_due_at":{"type":"string","format":"date-time","maxLength":128}}),
        json!(["sources", "evidence_basis"]),
    )
}
fn planned_common(operation_value: Value, extra: Value, required_extra: &[&str]) -> Value {
    let mut fields = json!({"operation_id":uuid(),"unit_id":uuid(),"client_label":text(128),"operation":operation_value,"replacement_bindings":{"type":"array","items":super::knowledge_document_schema::binding(),"maxItems":LIST},"reason":text(4096),"authority_basis":text(4096),"dependency_operation_ids":unique_uuids(OPERATIONS),"binding_pins":{"type":"array","maxItems":0}});
    fields
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    let mut required = vec![
        "operation_id",
        "unit_id",
        "client_label",
        "operation",
        "reason",
        "authority_basis",
        "dependency_operation_ids",
        "replacement_bindings",
    ];
    required.extend_from_slice(required_extra);
    object_schema(fields, json!(required))
}
fn planned_operation() -> Value {
    let expected = json!({"expected_revision":revision(),"expected_lifecycle":lifecycle()});
    let document = super::knowledge_document_schema::document();
    let bindings =
        json!({"type":"array","items":super::knowledge_document_schema::binding(),"maxItems":LIST});
    json!({"oneOf":[
        planned_common(json!({"const":"create"}),json!({"document":document}), &["document"]),
        planned_common(json!({"const":"revise"}),merge(expected.clone(),json!({"document":super::knowledge_document_schema::document()})), &["expected_revision","expected_lifecycle","document"]),
        planned_common(json!({"const":"revalidate"}),merge(expected.clone(),json!({"revalidation":revalidation()})), &["expected_revision","expected_lifecycle","revalidation"]),
        planned_common(json!({"const":"supersede"}),merge(expected.clone(),json!({"successor":successor(),"replacement_bindings":merge(bindings.clone(),json!({"minItems":1}))})), &["expected_revision","expected_lifecycle","successor","replacement_bindings"]),
        planned_common(json!({"enum":["retract","erase"]}),expected, &["expected_revision","expected_lifecycle"])
    ]})
}
fn merge(mut left: Value, right: Value) -> Value {
    left.as_object_mut()
        .unwrap()
        .extend(right.as_object().unwrap().clone());
    left
}
fn method_read() -> Value {
    object_schema(
        json!({"instruction_id":text(256),"version":text(128),"digest":digest()}),
        json!(["instruction_id", "version", "digest"]),
    )
}
fn obligation_receipt() -> Value {
    object_schema(
        json!({"operation_id":uuid(),"profile_id":profile(),"obligation_id":text(256),"disposition":{"enum":["satisfied","reused","not_applicable","unresolved"]},"reason":text(4096),"changeset_digest":digest(),"method_reads":array(method_read(),LIST),"input_digests":strings(256,LIST),"reused_receipt_id":uuid()}),
        json!([
            "operation_id",
            "profile_id",
            "obligation_id",
            "disposition",
            "reason",
            "changeset_digest",
            "method_reads",
            "input_digests"
        ]),
    )
}
fn impact_target() -> Value {
    object_schema(
        json!({"reference":text(4096),"owner_ref":text(1024),"effect":text(4096),"blocking":{"type":"boolean"}}),
        json!(["reference", "owner_ref", "effect", "blocking"]),
    )
}
pub(super) fn review_finding() -> Value {
    object_schema(
        json!({"id":text(256),"summary":text(4096),"owner_ref":text(1024),"revisit_phase_id":phase_id(),"closed":{"type":"boolean"},"closure_output_digest":digest()}),
        json!(["id", "summary", "owner_ref", "revisit_phase_id", "closed"]),
    )
}
pub(super) fn effect() -> Value {
    object_schema(
        json!({"effect_id":uuid(),"kind":{"enum":["exact_delivery","invalidation","impact","search","visibility_closure","owned_copy_purge","backup_disposition"]},"status":{"enum":["not_applicable","not_configured","pending","ready","failed"]},"generation":generation(),"owner_ref":text(1024),"detail":bounded(4096)}),
        json!([
            "effect_id",
            "kind",
            "status",
            "generation",
            "owner_ref",
            "detail"
        ]),
    )
}
fn tagged(phase: &str, fields: Value, required: Value) -> Value {
    object_schema(
        json!({"phase":{"const":phase},"data":object_schema(fields,required)}),
        json!(["phase", "data"]),
    )
}

pub(super) fn phase_data() -> Value {
    json!({"oneOf":[
        tagged("kc-intake",json!({"bounded_outcome":text(262144),"operation_hints":array(super::operation_hint(),OPERATIONS),"authority_boundary":text(4096),"completion":completion()}),json!(["bounded_outcome","operation_hints","authority_boundary","completion"])),
        tagged("kc-resolve-baseline",json!({"workspace_generation":generation(),"registry_generation":generation(),"policy_generation":generation(),"targets":array(guard(),OPERATIONS),"dependencies":array(guard(),OPERATIONS),"identity_matches":array(identity_match(),OPERATIONS),"source_availability":strings(4096,LIST),"assessment_conflicts":strings(4096,LIST),"assessment_gaps":strings(4096,LIST),"conflicts":strings(4096,LIST),"missing_context":strings(4096,LIST),"digest":computed_digest()}),json!(["workspace_generation","registry_generation","policy_generation","targets","dependencies","identity_matches","source_availability","conflicts","missing_context","digest"])),
        tagged("kc-qualify-plan",json!({"operations":{"type":"array","items":qualification(),"minItems":1,"maxItems":OPERATIONS}}),json!(["operations"])),
        tagged("kc-qualify-evidence",json!({"claims":array(evidence_claim(),OPERATIONS),"source_pins":array(source_pin(),LIST),"source_pin_digest":computed_digest(),"unresolved_gaps":strings(4096,LIST)}),json!(["claims","source_pins","source_pin_digest","unresolved_gaps"])),
        tagged("kc-prepare-change",json!({"revision":revision(),"operations":{"type":"array","items":planned_operation(),"minItems":1,"maxItems":OPERATIONS},"semantic_diff":text(262144),"evidence_digest":computed_digest(),"digest":computed_digest()}),json!(["revision","operations","semantic_diff","evidence_digest","digest"])),
        tagged("kc-domain-checks",json!({"receipts":array(obligation_receipt(),OBLIGATIONS),"unresolved_obligation_ids":strings(256,OBLIGATIONS)}),json!(["receipts","unresolved_obligation_ids"])),
        tagged("kc-impact-plan",json!({"synchronous_changes":strings(4096,4096),"affected_contexts":array(impact_target(),4096),"derivations":array(impact_target(),4096),"owned_copies":array(impact_target(),4096),"followups":array(impact_target(),4096),"blocking_conflicts":strings(4096,4096),"digest":computed_digest()}),json!(["synchronous_changes","affected_contexts","derivations","owned_copies","followups","blocking_conflicts","digest"])),
        tagged("kc-review-reconcile",json!({"outcome":{"enum":["ready","no_change","rejected","findings"]},"reviewed_digests":strings(256,LIST),"covered_operation_ids":array(uuid(),OPERATIONS),"covered_obligation_ids":strings(256,OBLIGATIONS),"findings":array(review_finding(),LIST),"summary":text(262144)}),json!(["outcome","reviewed_digests","covered_operation_ids","covered_obligation_ids","findings","summary"])),
        tagged("kc-result-handoff",json!({"canonical":{"enum":["not_applied","applied","no_change","rejected"]},"user_outcome":{"enum":["achieved","not_achieved","partial"]},"summary":text(262144),"remaining_work":strings(4096,LIST),"publisher_receipt_id":uuid(),"effects":array(effect(),LIST)}),json!(["canonical","user_outcome","summary","remaining_work","effects"]))
    ]})
}
