use super::*;

pub(crate) fn validate_artifact_definition(phase: &PipelinePhaseDefinition) -> Result<()> {
    let mut identities = BTreeSet::new();
    for (index, requirement) in phase.required_artifacts.iter().enumerate() {
        let path = format!("required_artifacts[{index}]");
        let fail = |rule, field: &str, expected: &str, actual: &str| {
            artifact_refusal(
                RefusalCode::InputSchemaInvalid,
                rule,
                format!("{path}.{field}"),
                expected,
                actual,
            )
        };
        let schema_valid = match (
            &requirement.schema_ref,
            &requirement.schema_resource_id,
            &requirement.schema_resource_digest,
        ) {
            (None, None, None) => requirement.media_type != "application/json",
            (Some(schema_ref), Some(resource_id), Some(resource_digest)) => {
                !schema_ref.trim().is_empty()
                    && !resource_id.trim().is_empty()
                    && !resource_digest.trim().is_empty()
                    && schema_ref.contains(resource_digest)
                    && phase.resources.iter().any(|resource| {
                        resource.id == *resource_id
                            && resource.digest == *resource_digest
                            && resource
                                .origin_refs
                                .iter()
                                .any(|origin| schema_ref.starts_with(origin))
                    })
            }
            _ => false,
        };
        if !valid_pattern(&requirement.name_pattern) {
            return Err(fail(
                "WP6-ARTIFACT-DEFINITION-NAME",
                "name_pattern",
                "safe relative pattern with at most one wildcard",
                "invalid pattern",
            ));
        }
        if requirement.media_type.trim().is_empty() {
            return Err(fail(
                "WP6-ARTIFACT-DEFINITION-MEDIA",
                "media_type",
                "nonblank media type",
                "blank",
            ));
        }
        if !schema_valid {
            return Err(fail(
                "WP6-ARTIFACT-DEFINITION-SCHEMA",
                "schema_ref",
                "JSON schema resource trio with matching digest and origin; optional for other media",
                "missing, incomplete or mismatched",
            ));
        }
        if requirement.minimum_matches == 0 {
            return Err(fail(
                "WP6-ARTIFACT-DEFINITION-COUNT",
                "minimum_matches",
                "positive minimum count",
                "0",
            ));
        }
        if !identities.insert((
            &requirement.name_pattern,
            &requirement.media_type,
            &requirement.when_verdict,
        )) {
            return Err(fail(
                "WP6-ARTIFACT-DEFINITION-DUPLICATE",
                "name_pattern",
                "unique name/media/verdict requirement",
                "duplicate",
            ));
        }
        if requirement.when_verdict.as_ref().is_some_and(|verdict| {
            !phase
                .allowed_verdicts
                .iter()
                .any(|allowed| allowed == verdict)
        }) {
            return Err(fail(
                "WP6-ARTIFACT-DEFINITION-VERDICT",
                "when_verdict",
                "allowed phase verdict",
                "unknown verdict",
            ));
        }
    }
    validate_validator_definition(phase)?;
    Ok(())
}

pub(super) fn validate_validator_definition(phase: &PipelinePhaseDefinition) -> Result<()> {
    let mut identities = BTreeSet::new();
    for (index, contract) in phase.validator_contracts.iter().enumerate() {
        let path = format!("validator_contracts[{index}]");
        let fail = |rule, field: &str, expected: &str, actual: &str| {
            artifact_refusal(
                RefusalCode::InputSchemaInvalid,
                rule,
                format!("{path}.{field}"),
                expected,
                actual,
            )
        };
        if contract.resource_id.trim().is_empty() {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-RESOURCE",
                "resource_id",
                "nonblank resource_id",
                "blank",
            ));
        }
        if contract.version.trim().is_empty() {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-VERSION",
                "version",
                "nonblank version",
                "blank",
            ));
        }
        if contract.digest.trim().is_empty() {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-DIGEST",
                "digest",
                "nonblank digest",
                "blank",
            ));
        }
        if contract.stage.trim().is_empty() {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-STAGE",
                "stage",
                "nonblank stage",
                "blank",
            ));
        }
        if contract.artifact_patterns.is_empty() {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-PATTERNS",
                "artifact_patterns",
                "nonempty list",
                "count=0",
            ));
        }
        if contract.success_verdicts.is_empty() {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-SUCCESS",
                "success_verdicts",
                "nonempty list",
                "count=0",
            ));
        }
        if let Some(item_index) = duplicate_index(contract.artifact_patterns.iter()) {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-PATTERN-DUPLICATE",
                &format!("artifact_patterns[{item_index}]"),
                "unique list entry",
                "duplicate",
            ));
        }
        if let Some(item_index) = duplicate_index(contract.required_verdicts.iter()) {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-REQUIRED-DUPLICATE",
                &format!("required_verdicts[{item_index}]"),
                "unique list entry",
                "duplicate",
            ));
        }
        if let Some(item_index) = duplicate_index(contract.success_verdicts.iter()) {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-SUCCESS-DUPLICATE",
                &format!("success_verdicts[{item_index}]"),
                "unique list entry",
                "duplicate",
            ));
        }
        if !identities.insert((&contract.resource_id, &contract.stage)) {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-DUPLICATE",
                "resource_id",
                "unique resource/stage identity",
                "duplicate",
            ));
        }
        for (pattern_index, pattern) in contract.artifact_patterns.iter().enumerate() {
            if !valid_pattern(pattern) {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-DEFINITION-PATTERN",
                    &format!("artifact_patterns[{pattern_index}]"),
                    "safe bounded artifact pattern",
                    "invalid pattern",
                ));
            }
            if !phase
                .required_artifacts
                .iter()
                .any(|requirement| requirement.name_pattern == *pattern)
            {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-DEFINITION-PATTERN-REQUIREMENT",
                    &format!("artifact_patterns[{pattern_index}]"),
                    "pattern declared by phase artifact requirement",
                    "undeclared",
                ));
            }
        }
        for (verdict_index, verdict) in contract.success_verdicts.iter().enumerate() {
            if !phase
                .allowed_verdicts
                .iter()
                .any(|allowed| allowed == verdict)
            {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-DEFINITION-SUCCESS-VERDICT",
                    &format!("success_verdicts[{verdict_index}]"),
                    "allowed phase verdict",
                    "unknown",
                ));
            }
        }
        for (verdict_index, verdict) in contract.required_verdicts.iter().enumerate() {
            if !phase
                .allowed_verdicts
                .iter()
                .any(|allowed| allowed == verdict)
            {
                return Err(fail(
                    "WP6-VALIDATOR-RECEIPT-DEFINITION-REQUIRED-VERDICT",
                    &format!("required_verdicts[{verdict_index}]"),
                    "allowed phase verdict",
                    "unknown",
                ));
            }
        }
        if !contract.required_verdicts.is_empty()
            && let Some(verdict_index) = contract
                .success_verdicts
                .iter()
                .position(|verdict| !contract.required_verdicts.contains(verdict))
        {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-SUCCESS-REQUIRED",
                &format!("success_verdicts[{verdict_index}]"),
                "success verdict included in required_verdicts",
                "absent",
            ));
        }
        if !phase.resources.iter().any(|resource| {
            resource.id == contract.resource_id
                && resource.version == contract.version
                && resource.digest == contract.digest
        }) {
            return Err(fail(
                "WP6-VALIDATOR-RECEIPT-DEFINITION-RESOURCE-MATCH",
                "resource_id",
                "phase resource with matching ID/version/digest",
                "absent or mismatched",
            ));
        }
    }
    Ok(())
}
