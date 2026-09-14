use crate::*;

pub(crate) fn validate_output_constraint(
    phase: &PipelinePhaseDefinition,
    constraint: &PipelineOutputConstraint,
) -> Result<()> {
    let valid_field = |field: &str| {
        !field.trim().is_empty() && phase.required_fields.iter().any(|value| value == field)
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
