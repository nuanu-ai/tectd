use crate::*;
use std::collections::BTreeSet;

mod definitions;
mod helpers;
mod receipts;
pub(crate) use definitions::validate_artifact_definition;
use helpers::*;
use receipts::validate_validator_receipts;

/// Resolve phase-local definition diagnostics at the caller that owns the phase index.
pub(crate) fn prefix_definition_refusal(mut error: Error, phase_index: usize) -> Error {
    fn prefix(error: &mut Error, phase_index: usize) {
        let refusal = match error {
            Error::Refused(refusal) => refusal,
            Error::PipelineRefused { source, refusal } => {
                prefix(source, phase_index);
                refusal
            }
            _ => return,
        };
        if let Some(path) = &mut refusal.path
            && (path.starts_with("required_artifacts[") || path.starts_with("validator_contracts["))
        {
            *path = format!("pipeline_definition.phases[{phase_index}].{path}");
        }
    }
    prefix(&mut error, phase_index);
    error
}

pub(crate) fn validate_artifacts(
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let mut names = BTreeSet::new();
    for (index, artifact) in output.artifacts.iter().enumerate() {
        let path = format!("output.artifacts[{index}]");
        let fail = |rule, field: &str, expected: &str, actual: &str| {
            artifact_refusal(
                RefusalCode::InvalidOutput,
                rule,
                format!("{path}.{field}"),
                expected,
                actual,
            )
        };
        if !valid_name(&artifact.name) {
            return Err(fail(
                "WP6-ARTIFACT-NAME",
                "name",
                "safe repository-relative artifact name",
                "unsafe name",
            ));
        }
        if artifact.media_type.trim().is_empty() {
            return Err(fail(
                "WP6-ARTIFACT-MEDIA",
                "media_type",
                "nonblank media type",
                "blank",
            ));
        }
        if artifact.body.is_empty() {
            return Err(fail(
                "WP6-ARTIFACT-BODY",
                "body",
                "nonempty artifact body",
                "length=0",
            ));
        }
        if artifact.digest.trim().is_empty() {
            return Err(fail(
                "WP6-ARTIFACT-DIGEST",
                "digest",
                "nonblank digest",
                "blank",
            ));
        }
        if artifact
            .reference
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(fail(
                "WP6-ARTIFACT-REFERENCE",
                "reference",
                "nonblank reference when present",
                "blank",
            ));
        }
        if !names.insert(&artifact.name) {
            return Err(fail(
                "WP6-ARTIFACT-DUPLICATE",
                "name",
                "unique artifact name",
                "duplicate",
            ));
        }
        let requirements = phase
            .required_artifacts
            .iter()
            .filter(|requirement| applies(requirement, output.verdict.as_deref()))
            .filter(|requirement| pattern_matches(&requirement.name_pattern, &artifact.name))
            .collect::<Vec<_>>();
        if requirements.is_empty() {
            return Err(fail(
                "WP6-ARTIFACT-REQUIREMENT",
                "name",
                "artifact matching an applicable phase requirement",
                "no matching requirement",
            ));
        }
        if requirements
            .iter()
            .any(|requirement| requirement.media_type != artifact.media_type)
        {
            return Err(fail(
                "WP6-ARTIFACT-REQUIREMENT-MEDIA",
                "media_type",
                "media type matching every applicable requirement",
                "mismatched",
            ));
        }
    }
    for (index, requirement) in
        phase
            .required_artifacts
            .iter()
            .enumerate()
            .filter(|(_, requirement)| {
                requirement.required && applies(requirement, output.verdict.as_deref())
            })
    {
        let matches = output
            .artifacts
            .iter()
            .filter(|artifact| pattern_matches(&requirement.name_pattern, &artifact.name))
            .count();
        if matches < requirement.minimum_matches as usize {
            return Err(artifact_refusal(
                RefusalCode::InvalidOutput,
                "WP6-ARTIFACT-REQUIRED-COUNT",
                "output.artifacts",
                format!(
                    "at least {} matches for required_artifacts[{index}]",
                    requirement.minimum_matches
                ),
                format!("count={matches}"),
            ));
        }
    }
    validate_validator_receipts(phase, output)?;
    Ok(())
}

#[cfg(test)]
mod tests;
