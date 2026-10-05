use crate::{
    engineering_review::{validate_completion_constraints, validate_definition_constraints},
    pipeline_artifacts::{validate_artifact_definition, validate_artifacts},
    pipeline_constraints::{output_constraint_satisfied, validate_output_constraint},
    pipeline_followups::{validate_followup_definitions, validate_followup_proposal},
    *,
};
use std::collections::BTreeSet;

fn schema_refusal(
    rule: &'static str,
    path: impl Into<String>,
    expected: impl Into<String>,
    actual: impl Into<String>,
) -> Error {
    let path = path.into();
    let (next_action, required) = if path.starts_with("pipeline_definition.") {
        ("select_valid_pinned_definition", "pipeline_definition")
    } else {
        ("correct_input_and_retry", "schema_valid_input")
    };
    Error::Refused(Box::new(
        Refusal::new(RefusalCode::InputSchemaInvalid)
            .with_message(RefusalCode::InputSchemaInvalid.message())
            .with_rule(rule)
            .with_path(path)
            .with_expected(expected)
            .with_actual(actual)
            .with_next_action(next_action)
            .with_required(required),
    ))
}

fn first_duplicate<T: Ord>(values: impl IntoIterator<Item = T>) -> Option<usize> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .enumerate()
        .find_map(|(index, value)| (!seen.insert(value)).then_some(index))
}

fn phase_ordinal(index: usize) -> Result<u32> {
    index
        .checked_add(1)
        .and_then(|ordinal| u32::try_from(ordinal).ok())
        .ok_or_else(|| {
            schema_refusal(
                "WP6-PHASE-ORDINAL-RANGE",
                format!("pipeline_definition.phases[{index}].ordinal"),
                "one-based ordinal representable as u32",
                "index outside ordinal range",
            )
        })
}

mod begin;
mod context_query;
mod definition;
mod escalation;
mod input_amendment;
#[cfg(test)]
use input_amendment::validate_source_amendment;

mod completion;
mod completion_helpers;
use completion_helpers::*;

#[cfg(test)]
mod source_amendment_tests;

#[cfg(test)]
mod remaining_diagnostic_tests;
