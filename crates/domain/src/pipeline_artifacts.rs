use crate::*;
use std::collections::BTreeSet;

pub(crate) fn validate_artifact_definition(phase: &PipelinePhaseDefinition) -> Result<()> {
    let mut identities = BTreeSet::new();
    for requirement in &phase.required_artifacts {
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
        if !valid_pattern(&requirement.name_pattern)
            || requirement.media_type.trim().is_empty()
            || !schema_valid
            || requirement.minimum_matches == 0
            || !identities.insert((
                &requirement.name_pattern,
                &requirement.media_type,
                &requirement.when_verdict,
            ))
            || requirement.when_verdict.as_ref().is_some_and(|verdict| {
                !phase
                    .allowed_verdicts
                    .iter()
                    .any(|allowed| allowed == verdict)
            })
        {
            return Err(Error::InvalidArguments);
        }
    }
    validate_validator_definition(phase)?;
    Ok(())
}

pub(crate) fn validate_artifacts(
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let mut names = BTreeSet::new();
    for artifact in &output.artifacts {
        if !valid_name(&artifact.name)
            || artifact.media_type.trim().is_empty()
            || artifact.body.is_empty()
            || artifact.digest.trim().is_empty()
            || artifact
                .reference
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            || !names.insert(&artifact.name)
        {
            return Err(Error::InvalidArguments);
        }
        let requirements = phase
            .required_artifacts
            .iter()
            .filter(|requirement| applies(requirement, output.verdict.as_deref()))
            .filter(|requirement| pattern_matches(&requirement.name_pattern, &artifact.name))
            .collect::<Vec<_>>();
        if requirements.is_empty()
            || requirements
                .iter()
                .any(|requirement| requirement.media_type != artifact.media_type)
        {
            return Err(Error::InvalidArguments);
        }
    }
    for requirement in phase.required_artifacts.iter().filter(|requirement| {
        requirement.required && applies(requirement, output.verdict.as_deref())
    }) {
        let matches = output
            .artifacts
            .iter()
            .filter(|artifact| pattern_matches(&requirement.name_pattern, &artifact.name))
            .count();
        if matches < requirement.minimum_matches as usize {
            return Err(Error::InvalidArguments);
        }
    }
    validate_validator_receipts(phase, output)?;
    Ok(())
}

fn validate_validator_definition(phase: &PipelinePhaseDefinition) -> Result<()> {
    let mut identities = BTreeSet::new();
    for contract in &phase.validator_contracts {
        if contract.resource_id.trim().is_empty()
            || contract.version.trim().is_empty()
            || contract.digest.trim().is_empty()
            || contract.stage.trim().is_empty()
            || contract.artifact_patterns.is_empty()
            || contract.success_verdicts.is_empty()
            || contract
                .artifact_patterns
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != contract.artifact_patterns.len()
            || contract
                .required_verdicts
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != contract.required_verdicts.len()
            || contract
                .success_verdicts
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != contract.success_verdicts.len()
            || !identities.insert((&contract.resource_id, &contract.stage))
            || contract.artifact_patterns.iter().any(|pattern| {
                !valid_pattern(pattern)
                    || !phase
                        .required_artifacts
                        .iter()
                        .any(|requirement| requirement.name_pattern == *pattern)
            })
            || contract.success_verdicts.iter().any(|verdict| {
                !phase
                    .allowed_verdicts
                    .iter()
                    .any(|allowed| allowed == verdict)
            })
            || contract.required_verdicts.iter().any(|verdict| {
                !phase
                    .allowed_verdicts
                    .iter()
                    .any(|allowed| allowed == verdict)
            })
            || !contract.required_verdicts.is_empty()
                && contract
                    .success_verdicts
                    .iter()
                    .any(|verdict| !contract.required_verdicts.contains(verdict))
            || !phase.resources.iter().any(|resource| {
                resource.id == contract.resource_id
                    && resource.version == contract.version
                    && resource.digest == contract.digest
            })
        {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(())
}

fn validate_validator_receipts(
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let receipt_ids = output
        .validator_receipts
        .iter()
        .map(|receipt| (&receipt.resource_id, &receipt.stage))
        .collect::<BTreeSet<_>>();
    if receipt_ids.len() != output.validator_receipts.len()
        || output.validator_receipts.iter().any(|receipt| {
            !phase.validator_contracts.iter().any(|contract| {
                receipt.resource_id == contract.resource_id && receipt.stage == contract.stage
            })
        })
    {
        return Err(Error::InvalidArguments);
    }
    for contract in &phase.validator_contracts {
        let receipt = output.validator_receipts.iter().find(|receipt| {
            receipt.resource_id == contract.resource_id && receipt.stage == contract.stage
        });
        let verdict = output.verdict.as_ref();
        let required = contract.required_verdicts.is_empty()
            || verdict.is_some_and(|verdict| contract.required_verdicts.contains(verdict));
        if required && receipt.is_none() {
            return Err(Error::InvalidArguments);
        }
        let Some(receipt) = receipt else { continue };
        let expected = output
            .artifacts
            .iter()
            .filter(|artifact| {
                contract
                    .artifact_patterns
                    .iter()
                    .any(|pattern| pattern_matches(pattern, &artifact.name))
            })
            .map(|artifact| (&artifact.name, &artifact.digest))
            .collect::<BTreeSet<_>>();
        let actual = receipt
            .artifacts
            .iter()
            .map(|artifact| (&artifact.name, &artifact.digest))
            .collect::<BTreeSet<_>>();
        let success = verdict.is_some_and(|verdict| contract.success_verdicts.contains(verdict));
        let not_run = receipt.command == "not_run";
        if receipt.version != contract.version
            || receipt.digest != contract.digest
            || receipt.command.trim().is_empty()
            || !not_run && (receipt.artifacts.len() != actual.len() || actual != expected)
            || not_run && (required || receipt.valid || !receipt.artifacts.is_empty())
            || success && (receipt.exit_code != 0 || !receipt.valid)
        {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(())
}

fn applies(requirement: &PipelineArtifactRequirement, verdict: Option<&str>) -> bool {
    requirement
        .when_verdict
        .as_deref()
        .is_none_or(|required| verdict == Some(required))
}

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

fn valid_pattern(value: &str) -> bool {
    let count = value.bytes().filter(|byte| *byte == b'*').count();
    if count > 1 {
        return false;
    }
    let without_wildcard = value.replace('*', "x");
    valid_name(&without_wildcard)
}

fn pattern_matches(pattern: &str, name: &str) -> bool {
    if !valid_name(name) {
        return false;
    }
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        let middle_end = name.len().saturating_sub(suffix.len());
        name.starts_with(prefix)
            && name.ends_with(suffix)
            && name.len() >= prefix.len() + suffix.len()
            && !name[prefix.len()..middle_end].contains('/')
    } else {
        pattern == name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_names_are_safe_and_patterns_are_bounded() {
        assert!(valid_pattern("decisions/*.md"));
        assert!(pattern_matches("decisions/*.md", "decisions/api.md"));
        assert!(!pattern_matches("decisions/*.md", "decisions/../api.md"));
        assert!(!valid_pattern("**/*.md"));
        assert!(!valid_name("../escape.json"));
    }
}
