use crate::*;

pub(crate) fn validate_output_constraint(
    phase: &PipelinePhaseDefinition,
    constraint: &PipelineOutputConstraint,
) -> Result<()> {
    let valid_field = |field: &str| {
        !field.trim().is_empty()
            && field.len() <= 256
            && (phase.required_fields.iter().any(|value| value == field)
                || phase.output_constraints.iter().any(|constraint| {
                    matches!(constraint, PipelineOutputConstraint::FieldRequired { field: declared, .. } if declared == field)
                }))
    };
    let valid_verdict = |verdict: &Option<String>| {
        verdict.as_ref().is_none_or(|value| {
            phase
                .allowed_verdicts
                .iter()
                .any(|allowed| allowed == value)
        })
    };
    let valid = match constraint {
        PipelineOutputConstraint::ResolvedKnowledgePublication { when_verdicts } => {
            !when_verdicts.is_empty()
                && when_verdicts.iter().all(|value| {
                    phase
                        .allowed_verdicts
                        .iter()
                        .any(|allowed| allowed == value)
                })
        }
        PipelineOutputConstraint::FieldRequired {
            field,
            when_verdict,
        } => !field.trim().is_empty() && field.len() <= 256 && valid_verdict(when_verdict),
        PipelineOutputConstraint::FieldEquals {
            field,
            value,
            when_verdict,
        }
        | PipelineOutputConstraint::FieldNotEquals {
            field,
            value,
            when_verdict,
        } => valid_field(field) && !value.is_empty() && valid_verdict(when_verdict),
        PipelineOutputConstraint::FieldIntegerEquals {
            field,
            when_verdict,
            ..
        }
        | PipelineOutputConstraint::FieldIntegerNotEquals {
            field,
            when_verdict,
            ..
        }
        | PipelineOutputConstraint::FieldIntegerMinimum {
            field,
            when_verdict,
            ..
        }
        | PipelineOutputConstraint::FieldBooleanEquals {
            field,
            when_verdict,
            ..
        } => valid_field(field) && valid_verdict(when_verdict),
        PipelineOutputConstraint::FieldOneOf {
            field,
            values,
            when_verdict,
        } => {
            valid_field(field)
                && !values.is_empty()
                && values.iter().all(|value| !value.is_empty())
                && valid_verdict(when_verdict)
        }
        PipelineOutputConstraint::FieldsEqual {
            field,
            other_field,
            when_verdict,
        } => valid_field(field) && valid_field(other_field) && valid_verdict(when_verdict),
    };
    if valid {
        Ok(())
    } else {
        Err(Error::InvalidArguments)
    }
}

pub(crate) fn output_constraint_satisfied(
    output: &PipelinePhaseOutputDraft,
    constraint: &PipelineOutputConstraint,
) -> bool {
    let applies = |when_verdict: &Option<String>| {
        when_verdict
            .as_ref()
            .is_none_or(|value| output.verdict.as_ref() == Some(value))
    };
    match constraint {
        PipelineOutputConstraint::ResolvedKnowledgePublication { when_verdicts } => {
            let required = output
                .verdict
                .as_ref()
                .is_some_and(|verdict| when_verdicts.contains(verdict));
            required == output.knowledge_publication.is_some()
        }
        PipelineOutputConstraint::FieldRequired {
            field,
            when_verdict,
        } => {
            !applies(when_verdict)
                || output
                    .fields
                    .get(field)
                    .is_some_and(|value| !value.trim().is_empty())
        }
        PipelineOutputConstraint::FieldEquals {
            field,
            value,
            when_verdict,
        } => !applies(when_verdict) || output.fields.get(field) == Some(value),
        PipelineOutputConstraint::FieldNotEquals {
            field,
            value,
            when_verdict,
        } => {
            !applies(when_verdict)
                || output
                    .fields
                    .get(field)
                    .is_some_and(|actual| actual != value)
        }
        PipelineOutputConstraint::FieldIntegerEquals {
            field,
            value,
            when_verdict,
        } => {
            !applies(when_verdict)
                || output
                    .fields
                    .get(field)
                    .and_then(|actual| actual.parse::<i64>().ok())
                    == Some(*value)
        }
        PipelineOutputConstraint::FieldIntegerNotEquals {
            field,
            value,
            when_verdict,
        } => {
            !applies(when_verdict)
                || output
                    .fields
                    .get(field)
                    .and_then(|actual| actual.parse::<i64>().ok())
                    .is_some_and(|actual| actual != *value)
        }
        PipelineOutputConstraint::FieldIntegerMinimum {
            field,
            value,
            when_verdict,
        } => {
            !applies(when_verdict)
                || output
                    .fields
                    .get(field)
                    .and_then(|actual| actual.parse::<i64>().ok())
                    .is_some_and(|actual| actual >= *value)
        }
        PipelineOutputConstraint::FieldBooleanEquals {
            field,
            value,
            when_verdict,
        } => {
            !applies(when_verdict)
                || output
                    .fields
                    .get(field)
                    .and_then(|actual| match actual.as_str() {
                        "true" => Some(true),
                        "false" => Some(false),
                        _ => None,
                    })
                    == Some(*value)
        }
        PipelineOutputConstraint::FieldOneOf {
            field,
            values,
            when_verdict,
        } => {
            !applies(when_verdict)
                || output
                    .fields
                    .get(field)
                    .is_some_and(|actual| values.contains(actual))
        }
        PipelineOutputConstraint::FieldsEqual {
            field,
            other_field,
            when_verdict,
        } => {
            !applies(when_verdict)
                || output.fields.contains_key(field)
                    && output.fields.get(field) == output.fields.get(other_field)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn phase(constraint: PipelineOutputConstraint) -> PipelinePhaseDefinition {
        PipelinePhaseDefinition {
            id: "r".into(),
            ordinal: 1,
            title: "Research".into(),
            required: true,
            disposition_required: false,
            instructions: vec![],
            skills: vec![],
            resources: vec![],
            required_artifacts: vec![],
            validator_contracts: vec![],
            required_fields: vec![],
            allowed_verdicts: vec!["ready".into(), "waiting_source".into()],
            required_dispositions: vec![],
            allowed_dispositions: vec![],
            output_constraints: vec![constraint],
            verdict_routes: vec![],
            followup_contracts: vec![],
            allowed_backward_to: vec![],
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "output".into(),
        }
    }

    fn output(verdict: &str, value: Option<&str>) -> PipelinePhaseOutputDraft {
        PipelinePhaseOutputDraft {
            body: "body".into(),
            producer_context_id: "context".into(),
            fields: value
                .map(|value| BTreeMap::from([("proof".into(), value.into())]))
                .unwrap_or_default(),
            verdict: Some(verdict.into()),
            dispositions: vec![],
            skill_reads: vec![],
            resource_reads: vec![],
            artifacts: vec![],
            validator_receipts: vec![],
            followup_proposal: None,
            reviewer_context: None,
            reference: None,
            knowledge_publication: None,
        }
    }

    #[test]
    fn conditional_required_field_is_exact() {
        let required = PipelineOutputConstraint::FieldRequired {
            field: "proof".into(),
            when_verdict: Some("ready".into()),
        };
        let phase = phase(required.clone());
        assert!(validate_output_constraint(&phase, &required).is_ok());
        assert!(!output_constraint_satisfied(
            &output("ready", None),
            &required
        ));
        assert!(!output_constraint_satisfied(
            &output("ready", Some("  ")),
            &required
        ));
        assert!(output_constraint_satisfied(
            &output("waiting_source", None),
            &required
        ));
        let undeclared = PipelineOutputConstraint::FieldEquals {
            field: "other".into(),
            value: "true".into(),
            when_verdict: Some("ready".into()),
        };
        assert_eq!(
            validate_output_constraint(&phase, &undeclared),
            Err(Error::InvalidArguments)
        );
    }
}
