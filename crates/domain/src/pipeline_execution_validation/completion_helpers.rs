use super::*;

pub(super) fn completion_refusal(
    code: RefusalCode,
    rule: &'static str,
    path: impl Into<String>,
    expected: impl Into<String>,
    actual: impl Into<String>,
    next_action: &'static str,
    required: impl Into<String>,
) -> Error {
    Error::Refused(Box::new(
        Refusal::new(code)
            .with_message(code.message())
            .with_rule(rule)
            .with_path(path)
            .with_expected(expected)
            .with_actual(actual)
            .with_next_action(next_action)
            .with_required(required),
    ))
}

pub(super) fn phase_read_receipt_refusal(
    kind: &str,
    rule: &'static str,
    path: &'static str,
    diff: &FullReceiptDiff,
) -> Result<Error> {
    let (next_action, required) = match kind {
        "skill" => ("supply_exact_phase_skill_reads", "exact_phase_skill_reads"),
        _ => (
            "supply_exact_phase_resource_reads",
            "exact_phase_resource_reads",
        ),
    };
    let (expected, actual) = diff
        .diagnostic_sections()
        .map_err(|_| Error::InternalInvariant)?;
    Ok(completion_refusal(
        RefusalCode::InvalidOutput,
        rule,
        path,
        expected,
        actual,
        next_action,
        required,
    ))
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
        .map(|value| bounded_refusal_detail(value.clone()))
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
    for (field, empty) in [
        ("summary", value.summary.trim().is_empty()),
        ("scope_impact", value.scope_impact.trim().is_empty()),
        ("remaining_work", value.remaining_work.trim().is_empty()),
        ("evidence", value.evidence.is_empty()),
    ] {
        if empty {
            return Err(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-COMPLETE-OUTPUT-20",
                format!("arguments.params.terminal_result.{field}"),
                "non-empty terminal result field",
                "empty",
                "supply_terminal_result_field",
                field,
            ));
        }
    }
    for (index, evidence) in value.evidence.iter().enumerate() {
        for (field, empty) in [
            ("kind", evidence.kind.trim().is_empty()),
            ("reference", evidence.reference.trim().is_empty()),
            ("observation", evidence.observation.trim().is_empty()),
        ] {
            if empty {
                return Err(completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-COMPLETE-OUTPUT-21",
                    format!("arguments.params.terminal_result.evidence[{index}].{field}"),
                    "non-empty evidence field",
                    "empty",
                    "supply_terminal_evidence",
                    "terminal_evidence",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_constraint_refusal_bounds_actual_at_utf8_boundaries() {
        let constraint = PipelineOutputConstraint::FieldEquals {
            field: "decision".to_owned(),
            value: "ready".to_owned(),
            when_verdict: None,
        };
        for value in [
            Some("no".to_owned()),
            Some("Ж🙂".to_owned()),
            Some("a".repeat(300)),
            Some(format!("{}🙂", "a".repeat(239))),
            Some(format!("a{}", "Ж".repeat(120))),
            None,
        ] {
            let fields = value.as_ref().map_or_else(
                || serde_json::json!({}),
                |value| serde_json::json!({"decision":value}),
            );
            let output: PipelinePhaseOutputDraft = serde_json::from_value(serde_json::json!({
                "producer_context_id":"test", "fields":fields
            }))
            .unwrap();
            let error = output_constraint_refusal(&constraint, &output);
            let Error::Refused(refusal) = error else {
                panic!("expected named refusal")
            };
            assert_eq!(refusal.code, RefusalCode::InvalidOutput);
            assert_eq!(refusal.rule.as_deref(), Some("WP6-OUTPUT-CONSTRAINT-01"));
            assert_eq!(
                refusal.path.as_deref(),
                Some("arguments.params.output.fields.decision")
            );
            assert_eq!(refusal.expected.as_deref(), Some("exact string `ready`"));
            assert_eq!(refusal.next_action.as_deref(), Some("correct_field"));
            assert_eq!(refusal.required.as_deref(), Some("decision"));
            let actual = refusal.actual.as_ref().unwrap();
            assert!(actual.len() <= 240);
            if let Some(value) = value {
                if value.len() <= 240 {
                    assert_eq!(actual, &value);
                } else {
                    assert!(actual.ends_with("...[truncated]"));
                    assert!(value.starts_with(actual.strip_suffix("...[truncated]").unwrap()));
                }
            } else {
                assert_eq!(actual, "missing");
            }
            let bytes = serde_json::to_vec(&refusal).unwrap();
            assert!(std::str::from_utf8(&bytes).is_ok());
            assert_eq!(serde_json::from_slice::<Refusal>(&bytes).unwrap(), *refusal);
        }
    }
}
