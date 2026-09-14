use crate::tools::object_schema;
use serde_json::{Value, json};

const LIST: usize = tect_domain::DK2_MAX_LIST_ITEMS;
const SOURCE_BYTES: usize = tect_domain::DK2_MAX_SOURCE_BYTES;

fn text(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max,
        "description":"The backend applies this limit to UTF-8 bytes; maxLength is the public structural bound."})
}
fn bounded(max: usize) -> Value {
    json!({"type":"string","maxLength":max,
        "description":"The backend applies this limit to UTF-8 bytes; this field may be empty."})
}
fn iri() -> Value {
    json!({"type":"string","minLength":1,"maxLength":4096,"pattern":"^(https?://|urn:)"})
}
fn strings(max: usize) -> Value {
    json!({"type":"array","items":text(max),"maxItems":LIST,"uniqueItems":true})
}
fn iris(min: usize) -> Value {
    json!({"type":"array","items":iri(),"minItems":min,"maxItems":LIST,"uniqueItems":true})
}
fn refs(min: usize) -> Value {
    json!({"type":"array","items":{"type":"integer","minimum":0,"maximum":127},
        "minItems":min,"maxItems":LIST,"uniqueItems":true})
}
fn u32s() -> Value {
    json!({"type":"array","items":{"type":"integer","minimum":0,"maximum":4294967295u64},
        "maxItems":LIST,"uniqueItems":true})
}
fn array(items: Value, min: usize) -> Value {
    json!({"type":"array","items":items,"minItems":min,"maxItems":LIST})
}

fn constraint() -> Value {
    object_schema(
        json!({"modality":{"enum":["must","must_not"]},"action":text(4096),"target_iri":iri()}),
        json!(["modality", "action", "target_iri"]),
    )
}
fn general() -> Value {
    object_schema(
        json!({"statement":text(SOURCE_BYTES),"assumptions":strings(4096),
            "evidence_scope":text(4096),"rationale":bounded(SOURCE_BYTES),
            "alternatives":strings(4096),"negative_limits":strings(4096),
            "unknown_limits":strings(4096)}),
        json!([
            "statement",
            "assumptions",
            "evidence_scope",
            "rationale",
            "alternatives",
            "negative_limits",
            "unknown_limits"
        ]),
    )
}
fn parameter() -> Value {
    object_schema(
        json!({"name":text(256),"description":text(4096),"required":{"type":"boolean"}}),
        json!(["name", "description", "required"]),
    )
}
fn step() -> Value {
    object_schema(
        json!({"ordinal":{"type":"integer","minimum":1,"maximum":4294967295u64},
            "action":text(4096),"expected_result":text(4096),"verification":text(4096)}),
        json!(["ordinal", "action", "expected_result", "verification"]),
    )
}
fn runbook() -> Value {
    object_schema(
        json!({"purpose_and_fit":text(4096),"target_environment_iris":iris(1),
            "parameters":array(parameter(),0),"prerequisites":strings(4096),
            "required_authority":text(4096),"steps":array(step(),1),
            "failure_and_recovery":text(SOURCE_BYTES),
            "proof_status":{"enum":["documented","static_verified","runtime_verified"]},
            "proof_evidence_refs":u32s(),"dependency_iris":iris(0)}),
        json!([
            "purpose_and_fit",
            "target_environment_iris",
            "parameters",
            "prerequisites",
            "required_authority",
            "steps",
            "failure_and_recovery",
            "proof_status",
            "proof_evidence_refs",
            "dependency_iris"
        ]),
    )
}
fn assertion() -> Value {
    object_schema(
        json!({"statement":text(SOURCE_BYTES),"observed":{"type":"boolean"},"evidence_refs":refs(1)}),
        json!(["statement", "observed", "evidence_refs"]),
    )
}
fn protocol() -> Value {
    object_schema(
        json!({"specification_uri":iri(),"specification_version":text(256),
            "provider_scope":merge_min(strings(4096),1),"network_scope":merge_min(strings(4096),1),
            "assertions":array(assertion(),1),"capabilities":strings(4096),
            "compatibility_constraints":strings(4096),"negative_states_and_quirks":strings(4096),
            "observation_bounds":text(4096)}),
        json!([
            "specification_uri",
            "specification_version",
            "provider_scope",
            "network_scope",
            "assertions",
            "capabilities",
            "compatibility_constraints",
            "negative_states_and_quirks",
            "observation_bounds"
        ]),
    )
}
fn observation() -> Value {
    object_schema(
        json!({"observed_at":{"type":"string","format":"date-time","maxLength":128},
            "status":text(4096),"limits":strings(4096),"evidence_refs":refs(1)}),
        json!(["observed_at", "status", "limits", "evidence_refs"]),
    )
}
fn devops() -> Value {
    object_schema(
        json!({"asset_iris":iris(1),"environment_iris":iris(1),"topology_links":strings(4096),
            "ownership":merge_min(strings(4096),1),"configuration_refs":strings(4096),
            "observations":array(observation(),1),"deployment_surfaces":merge_min(strings(4096),1),
            "configuration_custody":text(4096)}),
        json!([
            "asset_iris",
            "environment_iris",
            "topology_links",
            "ownership",
            "configuration_refs",
            "observations",
            "deployment_surfaces",
            "configuration_custody"
        ]),
    )
}
fn role() -> Value {
    object_schema(
        json!({"role":text(1024),"responsibilities":merge_min(strings(4096),1)}),
        json!(["role", "responsibilities"]),
    )
}
fn metric() -> Value {
    object_schema(
        json!({"name":text(1024),"meaning":text(4096),"objective":text(4096),
            "signal_source":text(4096)}),
        json!(["name", "meaning", "objective", "signal_source"]),
    )
}
fn operations() -> Value {
    object_schema(
        json!({"operating_purpose":text(SOURCE_BYTES),"roles":array(role(),1),"cadence":text(4096),
            "handoffs":strings(4096),"escalation":strings(4096),"status_semantics":strings(4096),
            "metrics":array(metric(),0),"signal_sources":strings(4096),"exceptions":strings(4096),
            "ownership_gaps":strings(4096)}),
        json!([
            "operating_purpose",
            "roles",
            "cadence",
            "handoffs",
            "escalation",
            "status_semantics",
            "metrics",
            "signal_sources",
            "exceptions",
            "ownership_gaps"
        ]),
    )
}
fn research_evidence() -> Value {
    object_schema(
        json!({"claim":text(SOURCE_BYTES),"source_refs":refs(1),"synthetic":{"type":"boolean"}}),
        json!(["claim", "source_refs", "synthetic"]),
    )
}
fn product_research() -> Value {
    object_schema(
        json!({"question":text(SOURCE_BYTES),"evidence_map":array(research_evidence(),1),
            "assumptions":strings(4096),"segments":strings(4096),"alternatives":strings(4096),
            "conclusions_and_decisions":merge_min(strings(SOURCE_BYTES),1),
            "observation_limits":strings(4096),"negative_evidence":strings(4096)}),
        json!([
            "question",
            "evidence_map",
            "assumptions",
            "segments",
            "alternatives",
            "conclusions_and_decisions",
            "observation_limits",
            "negative_evidence"
        ]),
    )
}
fn security() -> Value {
    object_schema(
        json!({"asset_iris":iris(1),"trust_boundaries":merge_min(strings(4096),1),
            "threats":merge_min(strings(4096),1),"controls":merge_min(strings(4096),1),
            "evidence_refs":refs(1),"verification_status":text(4096),
            "sensitivity":{"enum":["public","internal","sensitive","restricted"]},
            "applicable_authority":text(4096),"exceptions":strings(4096),"finding_state":text(4096),
            "remediation_proof_refs":refs(0)}),
        json!([
            "asset_iris",
            "trust_boundaries",
            "threats",
            "controls",
            "evidence_refs",
            "verification_status",
            "sensitivity",
            "applicable_authority",
            "exceptions",
            "finding_state",
            "remediation_proof_refs"
        ]),
    )
}
fn merge_min(mut value: Value, minimum: usize) -> Value {
    value["minItems"] = json!(minimum);
    value
}
pub(super) fn sections() -> Value {
    object_schema(
        json!({"constraint":constraint(),"general":general(),
            "runbook":runbook(),"protocol":protocol(),
            "devops":devops(),"operations":operations(),
            "product_research":product_research(),"security":security()}),
        json!([]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_sections_are_typed_and_omission_friendly() {
        let value = sections();
        assert_eq!(value["additionalProperties"], false);
        assert_eq!(value["required"], json!([]));
        assert_eq!(
            value["properties"]["runbook"]["properties"]["steps"]["minItems"],
            1
        );
        assert_eq!(
            value["properties"]["protocol"]["properties"]["assertions"]["minItems"],
            1
        );
        assert_eq!(
            value["properties"]["security"]["properties"]["sensitivity"]["enum"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
    }
}
