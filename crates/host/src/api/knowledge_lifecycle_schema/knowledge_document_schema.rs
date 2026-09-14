use crate::tools::object_schema;
use serde_json::{Value, json};

use super::knowledge_profile_schema;

const LIST: usize = tect_domain::DK2_MAX_LIST_ITEMS;
const SOURCE_BYTES: usize = tect_domain::DK2_MAX_SOURCE_BYTES;

fn text(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max,
        "description":"The backend applies this limit to UTF-8 bytes; maxLength is the public structural bound."})
}
fn iri() -> Value {
    json!({"type":"string","minLength":1,"maxLength":4096,"pattern":"^(https?://|urn:)"})
}
fn uuid() -> Value {
    json!({"type":"string","format":"uuid"})
}
fn strings(max: usize, min: usize) -> Value {
    json!({"type":"array","items":text(max),"minItems":min,"maxItems":LIST,"uniqueItems":true})
}
fn artifact() -> Value {
    object_schema(
        json!({"name":text(1024),"digest":text(256)}),
        json!(["name", "digest"]),
    )
}

fn planning_brief() -> Value {
    let selectors = object_schema(
        json!({
            "target_iris":{"type":"array","items":iri(),"maxItems":128,"uniqueItems":true},
            "environment_iris":{"type":"array","items":iri(),"maxItems":128,"uniqueItems":true},
            "action_classes":{"type":"array","items":text(1024),"maxItems":128,"uniqueItems":true}
        }),
        json!([]),
    );
    object_schema(
        json!({
            "local_id":text(128),
            "stage":{"enum":["program","scope","slice_candidates"]},
            "instruction":text(65536),
            "conditions":{"type":"array","items":text(4096),"maxItems":128,"uniqueItems":true},
            "exceptions":{"type":"array","items":text(4096),"maxItems":128,"uniqueItems":true},
            "purpose":text(4096),
            "selectors":selectors
        }),
        json!(["local_id", "stage", "instruction", "purpose"]),
    )
}

pub(super) fn source() -> Value {
    let evidence = json!({"enum":["document","declaration","observation","decision_record",
        "research","static_verification","runtime_verification","negative_evidence"]});
    let observed = json!({"type":"string","format":"date-time","maxLength":128});
    json!({"oneOf":[
        object_schema(json!({"kind":{"const":"snapshot"},"snapshot":object_schema(
            json!({"title":text(1024),"uri":iri(),"text":text(SOURCE_BYTES),
                "observed_at":observed.clone(),"evidence_kind":evidence.clone()}),
            json!(["title","uri","text","evidence_kind"]))}),json!(["kind","snapshot"])),
        object_schema(json!({"kind":{"const":"pipeline_output"},"output":object_schema(
            json!({"run_id":uuid(),"output_id":uuid(),"digest":text(256),
                "evidence_kind":evidence,"observed_at":observed,"evidence_scope":text(4096),
                "artifact":artifact()}),
            json!(["run_id","output_id","digest","evidence_kind","evidence_scope"]))}),
            json!(["kind","output"]))
    ]})
}

fn target() -> Value {
    json!({"oneOf":[
        object_schema(json!({"kind":{"const":"workspace"}}),json!(["kind"])),
        object_schema(json!({"kind":{"const":"program"},"program_id":uuid()}),json!(["kind","program_id"])),
        object_schema(json!({"kind":{"const":"scope"},"scope_id":uuid()}),json!(["kind","scope_id"])),
        object_schema(json!({"kind":{"const":"slice"},"scope_id":uuid(),"slice_id":uuid()}),json!(["kind","scope_id","slice_id"])),
        object_schema(json!({"kind":{"const":"slice_phase"},"scope_id":uuid(),"slice_id":uuid(),
            "phase_id":text(256)}),json!(["kind","scope_id","slice_id","phase_id"]))
    ]})
}
fn version() -> Value {
    json!({"oneOf":[
        object_schema(json!({"kind":{"const":"current_accepted"}}),json!(["kind"])),
        object_schema(json!({"kind":{"const":"pinned_revision"},"revision":{"type":"integer","minimum":1}}),
            json!(["kind","revision"]))
    ]})
}
pub(super) fn binding() -> Value {
    object_schema(
        json!({"target":target(),"purpose":{"enum":["required","reference","procedure","proof_basis"]},
            "version_resolution":version()}),
        json!(["target", "purpose", "version_resolution"]),
    )
}

pub(super) fn document() -> Value {
    object_schema(
        json!({"title":text(1024),"canonical_text":text(SOURCE_BYTES),
            "knowledge_kind":{"enum":["constraint","claim","decision","hypothesis","procedure",
                "protocol","infrastructure","operating_model","product_research","security"]},
            "epistemic_state":{"enum":["normative","decision","declared","observed","hypothesis",
                "negative_knowledge"]},"target_iris":{"type":"array","items":iri(),"minItems":1,
                "maxItems":LIST,"uniqueItems":true},"conditions":strings(4096,0),"exceptions":strings(4096,0),
            "sources":{"type":"array","items":source(),"minItems":1,"maxItems":LIST},
            "bindings":{"type":"array","items":binding(),"minItems":1,"maxItems":LIST},
            "profiles":{"type":"array","items":{"enum":["general","runbook","protocol","devops",
                "operations","product_research","security"]},"minItems":1,"maxItems":7,"uniqueItems":true},
            "access_scope":{"enum":["workspace_members","owners_only"]},"owner_ref":text(1024),
            "authority_basis":text(4096),
            "valid_from":{"type":"string","format":"date-time","maxLength":128},
            "valid_until":{"type":"string","format":"date-time","maxLength":128},
            "review_due_at":{"type":"string","format":"date-time","maxLength":128},
            "planning_briefs":{"type":"array","items":planning_brief(),"maxItems":tect_domain::PLANNING_KNOWLEDGE_MAX_BRIEFS,"uniqueItems":true},
            "sections":knowledge_profile_schema::sections()}),
        json!([
            "title",
            "canonical_text",
            "knowledge_kind",
            "epistemic_state",
            "target_iris",
            "conditions",
            "exceptions",
            "sources",
            "bindings",
            "profiles",
            "access_scope",
            "owner_ref",
            "authority_basis",
            "sections"
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_sources_and_bindings_are_closed_typed_objects() {
        let value = document();
        assert_eq!(value["additionalProperties"], false);
        assert!(
            !value["required"]
                .as_array()
                .unwrap()
                .contains(&json!("valid_from"))
        );
        assert_eq!(value["properties"]["sources"]["minItems"], 1);
        assert_eq!(source()["oneOf"].as_array().unwrap().len(), 2);
        assert_eq!(
            binding()["properties"]["target"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
        assert_eq!(
            value["properties"]["sections"]["additionalProperties"],
            false
        );
        assert_eq!(
            source()["oneOf"][0]["properties"]["snapshot"]["properties"]["observed_at"]["type"],
            "string"
        );
        assert_eq!(value["properties"]["valid_from"]["type"], "string");
    }
}
