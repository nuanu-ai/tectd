use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_application::NativePlanningGuidance;
use tect_domain::{
    CandidateMethodSnapshot, CandidateRuleSnapshot, Result, ScopeOpenBasis, SlicePlanningInput,
    SlicePlanningSnapshotMaterial, SliceResult,
};

pub(crate) const METHOD_ID: &str = "tectd-slice-candidates";
pub(crate) const METHOD_REVISION: &str = "1";
pub(crate) const METHOD_BODY: &str =
    include_str!("../../../skills/tectd-slice-candidates/SKILL.md");

pub(crate) fn method_snapshot() -> CandidateMethodSnapshot {
    CandidateMethodSnapshot {
        id: METHOD_ID.into(),
        revision: METHOD_REVISION.into(),
        digest: digest(METHOD_BODY.as_bytes()),
        body: METHOD_BODY.into(),
        origin_refs: vec![format!(
            "skills/tectd-slice-candidates/SKILL.md@{METHOD_REVISION}"
        )],
    }
}

pub(crate) fn rules() -> Result<Vec<CandidateRuleSnapshot>> {
    crate::scope_guidance::slice_candidate_rules()
}

pub(crate) fn help() -> Result<Value> {
    Ok(json!({
        "method": method_snapshot(),
        "guidance_registry": crate::scope_guidance::help_registry()?,
        "pipeline_catalog": crate::slice_pipeline_catalog::value(),
    }))
}

pub(crate) struct StaticSliceGuidance;

impl NativePlanningGuidance for StaticSliceGuidance {
    fn snapshot(
        &self,
        _basis: &ScopeOpenBasis,
        _inputs: &[SlicePlanningInput],
        _results: &[SliceResult],
    ) -> Result<SlicePlanningSnapshotMaterial> {
        Ok(SlicePlanningSnapshotMaterial {
            method: method_snapshot(),
            registry_revision: crate::scope_guidance::REGISTRY_REVISION.into(),
            registry_digest: crate::scope_guidance::registry_digest_value()?,
            rules: rules()?,
            catalogue: crate::slice_pipeline_catalog::snapshot(),
        })
    }
}

fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slice_design_guidance_delivers_all_four_rules() {
        let rules = rules().unwrap();
        assert_eq!(rules.len(), 4);
        assert!(
            rules
                .iter()
                .all(|rule| rule.revision == "2" && !rule.text.trim().is_empty())
        );
        assert_eq!(method_snapshot().revision, "1");
    }
}
