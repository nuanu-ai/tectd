use crate::*;

pub(super) fn phase_read_receipt_details(
    expected: &[(&str, &str, &str)],
    actual: &[(&str, &str, &str)],
) -> (String, String) {
    for &(id, version, digest) in expected {
        if let Some((_, _, actual_digest)) = actual
            .iter()
            .copied()
            .find(|(actual_id, actual_version, _)| *actual_id == id && *actual_version == version)
            && actual_digest != digest
        {
            return (
                format!("{id}@{version} digest={digest}"),
                format!("{id}@{version} digest={actual_digest}"),
            );
        }
    }

    let missing = expected
        .iter()
        .copied()
        .filter(|receipt| !actual.contains(receipt))
        .take(3)
        .map(|(id, version, digest)| format!("digest={digest} for {id}@{version}"))
        .collect::<Vec<_>>()
        .join(", ");
    let unexpected = actual
        .iter()
        .copied()
        .filter(|receipt| !expected.contains(receipt))
        .take(3)
        .map(|(id, version, digest)| format!("digest={digest} for {id}@{version}"))
        .collect::<Vec<_>>()
        .join(", ");
    (missing, unexpected)
}

pub(super) fn phase_read_receipt_refusal(
    kind: &str,
    rule: &'static str,
    path: &'static str,
    expected_reads: &str,
    actual_reads: &str,
    submitted_count: usize,
    unique_count: usize,
) -> Error {
    let (next_action, required) = match kind {
        "skill" => ("supply_exact_phase_skill_reads", "exact_phase_skill_reads"),
        _ => (
            "supply_exact_phase_resource_reads",
            "exact_phase_resource_reads",
        ),
    };
    let expected = bounded_refusal_detail(format!(
        "exact pinned phase {kind} read receipts: [{expected_reads}]"
    ));
    let actual = bounded_refusal_detail(format!(
        "submitted {submitted_count} receipt(s) ({} unique, {} duplicate): [{actual_reads}]",
        unique_count,
        submitted_count.saturating_sub(unique_count)
    ));
    Error::refused_at(
        RefusalCode::InvalidOutput,
        rule,
        path,
        expected,
        actual,
        next_action,
        required,
    )
}

fn bounded_refusal_detail(value: String) -> String {
    const LIMIT: usize = 240;
    const SUFFIX: &str = "...[truncated]";
    if value.len() <= LIMIT {
        return value;
    }
    let mut bounded = String::new();
    for character in value.chars() {
        if bounded.len() + character.len_utf8() + SUFFIX.len() > LIMIT {
            break;
        }
        bounded.push(character);
    }
    bounded.push_str(SUFFIX);
    bounded
}

pub(super) fn output_constraint_refusal(
    constraint: &PipelineOutputConstraint,
    output: &PipelinePhaseOutputDraft,
) -> Error {
    let (field, expected, next_action) = match constraint {
        PipelineOutputConstraint::CommandReceipt {
            field,
            required_status,
            required_scope,
            require_nonzero_exit,
            target_field,
            ..
        } => (
            field,
            format!(
                "JSON string {{command,target,status:{required_status},exit_code:{},fresh:true,skipped:false,scopes:[...{required_scope}...]}}{}",
                if *require_nonzero_exit {
                    "nonzero"
                } else {
                    "0"
                },
                target_field
                    .as_ref()
                    .map(|target| format!(" with target equal to output.fields.{target}"))
                    .unwrap_or_default()
            ),
            "replace_command_receipt",
        ),
        PipelineOutputConstraint::ReviewerContextMode {
            field,
            independent_value,
            self_value,
        } => (
            field,
            format!(
                "{independent_value} with fresh independent reviewer_context, or {self_value} without reviewer_context"
            ),
            "correct_current_plan_review",
        ),
        PipelineOutputConstraint::FieldEquals { field, value, .. } => {
            (field, format!("exact string `{value}`"), "correct_field")
        }
        PipelineOutputConstraint::FieldOneOf { field, values, .. } => (
            field,
            format!("one of [{}]", values.join(", ")),
            "correct_field",
        ),
        PipelineOutputConstraint::FieldsEqual {
            field, other_field, ..
        } => (
            field,
            format!("same value as output.fields.{other_field}"),
            "correct_field",
        ),
        other => {
            return Error::Refused(Box::new(
                Refusal::new(RefusalCode::InvalidOutput)
                    .with_message(RefusalCode::InvalidOutput.message())
                    .with_rule("WP6-OUTPUT-CONSTRAINT-01")
                    .with_path("arguments.params.output.fields")
                    .with_expected(format!("current phase constraint {other:?}"))
                    .with_actual("constraint not satisfied")
                    .with_next_action("correct_phase_output")
                    .with_required("valid_output"),
            ));
        }
    };
    let actual = output
        .fields
        .get(field)
        .map(|value| {
            if value.len() > 240 {
                format!("{}...[truncated]", &value[..240])
            } else {
                value.clone()
            }
        })
        .unwrap_or_else(|| "missing".to_owned());
    Error::Refused(Box::new(
        Refusal::new(RefusalCode::InvalidOutput)
            .with_message(RefusalCode::InvalidOutput.message())
            .with_rule("WP6-OUTPUT-CONSTRAINT-01")
            .with_path(format!("arguments.params.output.fields.{field}"))
            .with_expected(expected)
            .with_actual(actual)
            .with_next_action(next_action)
            .with_required(field.clone()),
    ))
}

pub(super) fn missing_test_target(
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
) -> bool {
    phase.required_fields.iter().any(|key| {
        key.contains("test_target")
            && output
                .fields
                .get(key)
                .is_none_or(|value| value.trim().is_empty())
    })
}

pub(super) fn reject_agent_supplied_proof(request: &CompletePipelinePhase) -> Result<()> {
    if !request.consumed_outputs.is_empty() {
        return Err(Error::refused_backend_proof(
            "arguments.params.consumed_outputs",
        ));
    }
    if !request.consumed_inputs.is_empty() {
        return Err(Error::refused_backend_proof(
            "arguments.params.consumed_inputs",
        ));
    }
    if request.consumed_knowledge.is_some() {
        return Err(Error::refused_backend_proof(
            "arguments.params.consumed_knowledge",
        ));
    }
    if !request.output.skill_reads.is_empty() {
        return Err(Error::refused_backend_proof(
            "arguments.params.output.skill_reads",
        ));
    }
    if !request.output.resource_reads.is_empty() {
        return Err(Error::refused_backend_proof(
            "arguments.params.output.resource_reads",
        ));
    }
    Ok(())
}

pub(super) fn validate_terminal(value: &PipelineTerminalResultDraft) -> Result<()> {
    if value.summary.trim().is_empty()
        || value.scope_impact.trim().is_empty()
        || value.remaining_work.trim().is_empty()
        || value.evidence.is_empty()
        || value.evidence.iter().any(|e| {
            e.kind.trim().is_empty()
                || e.reference.trim().is_empty()
                || e.observation.trim().is_empty()
        })
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
