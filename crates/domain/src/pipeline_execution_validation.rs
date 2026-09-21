use crate::{
    engineering_review::{validate_completion_constraints, validate_definition_constraints},
    pipeline_artifacts::{validate_artifact_definition, validate_artifacts},
    pipeline_constraints::{output_constraint_satisfied, validate_output_constraint},
    pipeline_followups::{validate_followup_definitions, validate_followup_proposal},
    *,
};
use std::collections::BTreeSet;

impl PipelineDefinitionSnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.version.trim().is_empty()
            || self.digest.trim().is_empty()
            || self.completion_contract.trim().is_empty()
            || self.escalation_contract.trim().is_empty()
            || self.phases.is_empty()
            || !self.allowed_modes.contains(&self.default_mode)
        {
            return Err(Error::InvalidArguments);
        }
        validate_instruction(&self.overview)?;
        let mut ids = BTreeSet::new();
        for (index, phase) in self.phases.iter().enumerate() {
            let required_fields = phase.required_fields.iter().collect::<BTreeSet<_>>();
            let allowed_verdicts = phase.allowed_verdicts.iter().collect::<BTreeSet<_>>();
            let required_dispositions = phase.required_dispositions.iter().collect::<BTreeSet<_>>();
            let allowed_dispositions = phase.allowed_dispositions.iter().collect::<BTreeSet<_>>();
            let verdict_routes = phase.verdict_routes.iter().collect::<BTreeSet<_>>();
            if phase.id.trim().is_empty()
                || phase.title.trim().is_empty()
                || phase.output_contract.trim().is_empty()
                || phase.ordinal != u32::try_from(index + 1).map_err(|_| Error::InvalidArguments)?
                || !ids.insert(phase.id.clone())
                || phase.instructions.is_empty() && phase.skills.is_empty()
                || required_fields.len() != phase.required_fields.len()
                || allowed_verdicts.len() != phase.allowed_verdicts.len()
                || required_dispositions.len() != phase.required_dispositions.len()
                || allowed_dispositions.len() != phase.allowed_dispositions.len()
                || verdict_routes.len() != phase.verdict_routes.len()
                || phase.disposition_required && phase.allowed_dispositions.is_empty()
                || self.version.starts_with("0.7") && phase.required_fields.len() > 8
                || phase
                    .required_fields
                    .iter()
                    .chain(&phase.allowed_verdicts)
                    .chain(&phase.required_dispositions)
                    .chain(&phase.allowed_dispositions)
                    .any(|value| value.trim().is_empty())
                || !phase.allowed_dispositions.is_empty()
                    && phase
                        .required_dispositions
                        .iter()
                        .any(|value| !phase.allowed_dispositions.contains(value))
                || !phase.allowed_verdicts.is_empty()
                    && (phase.verdict_routes.is_empty()
                        || phase.allowed_verdicts.iter().any(|verdict| {
                            !phase
                                .verdict_routes
                                .iter()
                                .any(|route| &route.verdict == verdict)
                        }))
                || phase.verdict_routes.iter().any(|route| {
                    route.verdict.trim().is_empty()
                        || !phase.allowed_verdicts.contains(&route.verdict)
                        || route.dispositions.iter().collect::<BTreeSet<_>>().len()
                            != route.dispositions.len()
                        || route.revisit_to.iter().collect::<BTreeSet<_>>().len()
                            != route.revisit_to.len()
                        || !route.revisit_to.is_empty()
                            && (route.transition != PipelineTransition::Continue
                                || route
                                    .revisit_to
                                    .iter()
                                    .any(|id| !phase.allowed_backward_to.contains(id)))
                        || phase.disposition_required && route.dispositions.is_empty()
                        || route.dispositions.iter().any(|value| {
                            value.trim().is_empty()
                                || !phase.allowed_dispositions.is_empty()
                                    && !phase.allowed_dispositions.contains(value)
                        })
                        || self.version.starts_with("0.7")
                            && route.outcome == PipelinePhaseOutcome::Completed
                            && phase
                                .required_dispositions
                                .iter()
                                .any(|required| !route.dispositions.contains(required))
                        || match route.transition {
                            PipelineTransition::Complete => {
                                route.outcome != PipelinePhaseOutcome::Completed
                                    || phase.ordinal as usize != self.phases.len()
                            }
                            PipelineTransition::Block => {
                                route.outcome != PipelinePhaseOutcome::Blocked
                            }
                            PipelineTransition::Escalate => {
                                route.outcome == PipelinePhaseOutcome::WaitingInput
                            }
                            PipelineTransition::Continue => false,
                        }
                })
            {
                return Err(Error::InvalidArguments);
            }
            for instruction in phase
                .instructions
                .iter()
                .chain(&phase.skills)
                .chain(&phase.resources)
            {
                if self.version.starts_with("0.7") && instruction.body.len() > 4 * 1024 {
                    return Err(Error::refused_at(
                        RefusalCode::PayloadTooLarge,
                        "WP6-INSTRUCTION-SIZE-01",
                        "pipeline_definition.phases[].instructions[].body",
                        "at most 4096 UTF-8 bytes",
                        instruction.body.len().to_string(),
                        "reduce_instruction_body",
                        "instruction_body",
                    ));
                }
                validate_instruction(instruction)?;
            }
            validate_artifact_definition(phase)?;
            validate_followup_definitions(self, phase)?;
            for constraint in &phase.output_constraints {
                validate_output_constraint(phase, constraint)?;
            }
        }
        if self.phases.iter().any(|phase| {
            phase.allowed_backward_to.iter().any(|id| {
                self.phases
                    .iter()
                    .find(|candidate| &candidate.id == id)
                    .is_none_or(|target| target.ordinal >= phase.ordinal)
            })
        }) {
            return Err(Error::InvalidArguments);
        }
        validate_definition_constraints(self)?;
        Ok(())
    }
}

fn validate_instruction(value: &PipelineInstructionSnapshot) -> Result<()> {
    if value.id.trim().is_empty()
        || value.version.trim().is_empty()
        || value.digest.trim().is_empty()
        || value.body.trim().is_empty()
        || value.origin_refs.is_empty()
        || value
            .origin_refs
            .iter()
            .any(|value| value.trim().is_empty())
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

impl BeginPipelineRun {
    pub fn validate(&self, definition: &PipelineDefinitionSnapshot) -> Result<()> {
        let inquiry_valid = match definition.kind {
            PipelineKind::Research => self
                .inquiry
                .as_ref()
                .is_some_and(|inquiry| inquiry.require_research().is_ok()),
            PipelineKind::DeepBrainstorming => self
                .inquiry
                .as_ref()
                .is_some_and(|inquiry| inquiry.validate().is_ok() && inquiry.is_decision()),
            _ => self.inquiry.is_none(),
        };
        let checkpoint_valid = match definition.kind {
            PipelineKind::Research => self
                .source_checkpoint
                .as_ref()
                .is_none_or(|checkpoint| checkpoint.validate().is_ok()),
            _ => self.source_checkpoint.is_none(),
        };
        if self.request_id.is_nil()
            || self.scope_id.is_nil()
            || self.slice_id.is_nil()
            || self.slice_revision < 1
            || self.qualification_reason.trim().is_empty()
            || self.definition_version.as_deref().is_some_and(|version| {
                version.trim().is_empty() || version.len() > 128 || version != definition.version
            })
            || !definition
                .allowed_modes
                .contains(&self.delivery_mode.unwrap_or(definition.default_mode))
            || !inquiry_valid
            || !checkpoint_valid
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl PipelineRunContextQuery {
    pub fn validate(&self) -> Result<()> {
        let valid_selector = match self.view {
            PipelineRunContextView::Current => self.output_id.is_none() && self.digest.is_none(),
            PipelineRunContextView::Output => {
                self.output_id.is_some_and(|id| !id.is_nil())
                    && self
                        .digest
                        .as_ref()
                        .is_some_and(|value| !value.trim().is_empty())
            }
            PipelineRunContextView::DeliveryReceipt => {
                self.output_id.is_none() && self.digest.is_none()
            }
        };
        if self.run_id.is_nil() || !valid_selector {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl CompletePipelinePhase {
    pub fn validate(&self, definition: &PipelineDefinitionSnapshot) -> Result<()> {
        if definition.version.starts_with("0.7") {
            reject_agent_supplied_proof(self)?;
        }
        if self.request_id.is_nil()
            || self.run_id.is_nil()
            || self.run_revision < 1
            || !definition.version.starts_with("0.7") && self.output.body.trim().is_empty()
            || self.output.producer_context_id.trim().is_empty()
            || self.output.producer_context_id.len() > MAX_PIPELINE_CONTEXT_ID_BYTES
            || self
                .output
                .reference
                .as_ref()
                .is_some_and(|v| v.trim().is_empty())
            || self.consumed_outputs.iter().any(|value| {
                value.phase_id.trim().is_empty()
                    || value.output_revision < 1
                    || value.digest.trim().is_empty()
            })
            || self.consumed_inputs.iter().any(|value| {
                value.input_id.is_nil() || value.sequence < 1 || value.digest.trim().is_empty()
            })
        {
            return Err(Error::InvalidArguments);
        }
        let phase = definition
            .phases
            .iter()
            .find(|phase| phase.id == self.phase_id)
            .ok_or(Error::InvalidArguments)?;
        let creates_checkpoint = definition.kind == PipelineKind::DeepBrainstorming
            && phase.ordinal == 5
            && self.output.verdict.as_deref() == Some("waiting_research");
        if creates_checkpoint != self.research_checkpoint.is_some() {
            return Err(Error::InvalidArguments);
        }
        if let Some(checkpoint) = &self.research_checkpoint {
            checkpoint.validate()?;
            if definition.kind != PipelineKind::DeepBrainstorming
                || phase.ordinal != 5
                || self.outcome != PipelinePhaseOutcome::WaitingInput
                || self.transition != PipelineTransition::Continue
                || self.output.verdict.as_deref() != Some("waiting_research")
            {
                return Err(Error::InvalidArguments);
            }
        }
        if let Some(field) = phase.required_fields.iter().find(|key| {
            self.output
                .fields
                .get(*key)
                .is_none_or(|v| v.trim().is_empty())
        }) {
            return Err(Error::Refused(Box::new(
                Refusal::new(RefusalCode::InvalidOutput)
                    .with_message(RefusalCode::InvalidOutput.message())
                    .with_rule("WP6-OUTPUT-FIELD-01")
                    .with_path(format!("arguments.params.output.fields.{field}"))
                    .with_expected("non-empty string required by the current phase contract")
                    .with_actual("missing or empty")
                    .with_next_action("supply_required_phase_field")
                    .with_required(field.clone()),
            )));
        }
        if (!phase.allowed_verdicts.is_empty()
            && self
                .output
                .verdict
                .as_ref()
                .is_none_or(|v| !phase.allowed_verdicts.contains(v)))
            || (!definition.version.starts_with("0.7")
                || phase.verdict_routes.is_empty()
                || self.outcome == PipelinePhaseOutcome::Completed)
                && phase
                    .required_dispositions
                    .iter()
                    .any(|required| !self.output.dispositions.contains(required))
            || phase.disposition_required && self.output.dispositions.is_empty()
            || !phase.allowed_dispositions.is_empty()
                && self
                    .output
                    .dispositions
                    .iter()
                    .any(|value| !phase.allowed_dispositions.contains(value))
            || !phase.allowed_dispositions.is_empty()
                && phase.required_dispositions.is_empty()
                && phase.verdict_routes.is_empty()
                && phase.disposition_required
                && self.output.dispositions.len() != 1
            || self
                .output
                .dispositions
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.output.dispositions.len()
            || self.output.reviewer_context.as_ref().is_some_and(|v| {
                v.reviewer_identity.trim().is_empty()
                    || v.reviewer_identity.len() > MAX_PIPELINE_CONTEXT_ID_BYTES
                    || v.reviewer_context_id.trim().is_empty()
                    || v.reviewer_context_id.len() > MAX_PIPELINE_CONTEXT_ID_BYTES
                    || v.reviewer_context_id != self.output.producer_context_id
                    || v.producer_context_ids.is_empty()
                    || v.producer_context_ids.len() > 100
                    || v.producer_context_ids
                        .iter()
                        .any(|id| id.trim().is_empty() || id.len() > MAX_PIPELINE_CONTEXT_ID_BYTES)
                    || v.producer_context_ids.contains(&v.reviewer_context_id)
                    || v.producer_context_ids.iter().collect::<BTreeSet<_>>().len()
                        != v.producer_context_ids.len()
                    || !v.fresh_input
            })
            || phase.fresh_reviewer_input && self.output.reviewer_context.is_none()
        {
            if definition.version.starts_with("0.7") && missing_test_target(phase, &self.output) {
                return Err(Error::refused_at(
                    RefusalCode::NoTestTarget,
                    "WP6-TEST-TARGET-01",
                    "arguments.params.output.fields.selected_test_target",
                    "a non-empty executable test target",
                    "missing or empty",
                    "select_test_target",
                    "selected_test_target",
                ));
            }
            return Err(Error::InvalidArguments);
        }
        for constraint in &phase.output_constraints {
            if !output_constraint_satisfied(&self.output, constraint) {
                return Err(output_constraint_refusal(constraint, &self.output));
            }
        }
        validate_completion_constraints(self, definition, phase)?;
        validate_artifacts(phase, &self.output)?;
        if let Some(verdict) = &self.output.verdict {
            let route = phase
                .verdict_routes
                .iter()
                .find(|route| {
                    &route.verdict == verdict
                        && route.outcome == self.outcome
                        && route.transition == self.transition
                        && match &self.revisit_phase_id {
                            Some(id) => route.revisit_to.contains(id),
                            None => route.revisit_to.is_empty(),
                        }
                })
                .ok_or(Error::InvalidArguments)?;
            let actual = self.output.dispositions.iter().collect::<BTreeSet<_>>();
            let expected = route.dispositions.iter().collect::<BTreeSet<_>>();
            if actual != expected {
                return Err(Error::InvalidArguments);
            }
        }
        validate_followup_proposal(
            definition,
            phase,
            &self.output,
            self.outcome,
            self.transition,
            &self.consumed_outputs,
        )?;
        let reads = self
            .output
            .skill_reads
            .iter()
            .map(|read| (&read.instruction_id, &read.version, &read.digest))
            .collect::<BTreeSet<_>>();
        let expected_reads = phase
            .skills
            .iter()
            .map(|skill| (&skill.id, &skill.version, &skill.digest))
            .collect::<BTreeSet<_>>();
        if reads != expected_reads || reads.len() != self.output.skill_reads.len() {
            let expected_values = expected_reads
                .iter()
                .map(|(id, version, digest)| (id.as_str(), version.as_str(), digest.as_str()))
                .collect::<Vec<_>>();
            let actual_values = reads
                .iter()
                .map(|(id, version, digest)| (id.as_str(), version.as_str(), digest.as_str()))
                .collect::<Vec<_>>();
            let (expected, actual) = phase_read_receipt_details(&expected_values, &actual_values);
            return Err(phase_read_receipt_refusal(
                "skill",
                "WP6-SKILL-READ-01",
                "arguments.params.output.skill_reads",
                &expected,
                &actual,
                self.output.skill_reads.len(),
                reads.len(),
            ));
        }
        let resource_reads = self
            .output
            .resource_reads
            .iter()
            .map(|read| (&read.instruction_id, &read.version, &read.digest))
            .collect::<BTreeSet<_>>();
        let expected_resource_reads = phase
            .resources
            .iter()
            .map(|resource| (&resource.id, &resource.version, &resource.digest))
            .collect::<BTreeSet<_>>();
        if resource_reads != expected_resource_reads
            || resource_reads.len() != self.output.resource_reads.len()
        {
            let expected_values = expected_resource_reads
                .iter()
                .map(|(id, version, digest)| (id.as_str(), version.as_str(), digest.as_str()))
                .collect::<Vec<_>>();
            let actual_values = resource_reads
                .iter()
                .map(|(id, version, digest)| (id.as_str(), version.as_str(), digest.as_str()))
                .collect::<Vec<_>>();
            let (expected, actual) = phase_read_receipt_details(&expected_values, &actual_values);
            return Err(phase_read_receipt_refusal(
                "resource",
                "WP6-RESOURCE-READ-01",
                "arguments.params.output.resource_reads",
                &expected,
                &actual,
                self.output.resource_reads.len(),
                resource_reads.len(),
            ));
        }
        match self.transition {
            PipelineTransition::Continue => {
                if self.terminal_result.is_some() || self.escalation_target.is_some() {
                    return Err(Error::InvalidArguments);
                }
            }
            PipelineTransition::Complete => {
                if self.terminal_result.is_none() || self.escalation_target.is_some() {
                    return Err(Error::InvalidArguments);
                }
            }
            PipelineTransition::Block => {
                if self.escalation_target.is_some()
                    || self.publish_blocked_result != self.terminal_result.is_some()
                {
                    return Err(Error::InvalidArguments);
                }
            }
            PipelineTransition::Escalate => {
                if self.terminal_result.is_none() || self.escalation_target.is_none() {
                    return Err(Error::InvalidArguments);
                }
            }
        }
        if self.publish_blocked_result
            && (self.outcome != PipelinePhaseOutcome::Blocked
                || self.transition != PipelineTransition::Block)
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(result) = &self.terminal_result {
            validate_terminal(result)?;
        }
        Ok(())
    }
}

fn phase_read_receipt_details(
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

fn phase_read_receipt_refusal(
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

fn output_constraint_refusal(
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

fn missing_test_target(phase: &PipelinePhaseDefinition, output: &PipelinePhaseOutputDraft) -> bool {
    phase.required_fields.iter().any(|key| {
        key.contains("test_target")
            && output
                .fields
                .get(key)
                .is_none_or(|value| value.trim().is_empty())
    })
}

fn reject_agent_supplied_proof(request: &CompletePipelinePhase) -> Result<()> {
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

fn validate_terminal(value: &PipelineTerminalResultDraft) -> Result<()> {
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

impl RecordPipelineInput {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.run_id.is_nil()
            || self.run_revision < 1
            || self.phase_id.trim().is_empty()
            || self.input.trim().is_empty()
            || self.input.len() > MAX_PIPELINE_INPUT_BYTES
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(amendment) = &self.source_amendment {
            validate_source_amendment(amendment)?;
        }
        Ok(())
    }
}

fn valid_relative_source_path(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= MAX_SOURCE_PATH_BYTES
        && !value.starts_with('/')
        && !value.contains(['\\', '\0'])
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

fn valid_media_type(value: &str) -> bool {
    value.trim() == value
        && value.len() <= 255
        && value.split_once('/').is_some_and(|(kind, subtype)| {
            !kind.is_empty()
                && !subtype.is_empty()
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-' | b'/'
                        )
                })
        })
}

fn validate_source_amendment(amendment: &PipelineSourceAmendment) -> Result<()> {
    let successor = &amendment.successor;
    let artifact = &successor.artifact;
    if amendment.target_phase_id.trim().is_empty()
        || amendment.predecessor.output_id.is_nil()
        || amendment.predecessor.output_revision < 1
        || amendment.predecessor.output_digest.trim().is_empty()
        || amendment.predecessor.output_digest.len() > 128
        || amendment.predecessor.artifact_name.trim().is_empty()
        || amendment.predecessor.artifact_name.len() > MAX_SOURCE_PATH_BYTES
        || amendment.predecessor.artifact_digest.trim().is_empty()
        || amendment.predecessor.artifact_digest.len() > 128
        || !valid_relative_source_path(&amendment.predecessor.source_path)
        || amendment.predecessor.source_digest.trim().is_empty()
        || amendment.predecessor.source_digest.len() > 128
        || !valid_relative_source_path(&successor.path)
        || !valid_relative_source_path(&artifact.name)
        || successor.path != artifact.name
        || !valid_media_type(&artifact.media_type)
        || artifact.body.trim().is_empty()
        || artifact.body.len() > MAX_PIPELINE_OUTPUT_BYTES
        || artifact.digest.len() != 64
        || !artifact
            .digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || artifact.reference.as_ref().is_some_and(|reference| {
            reference.trim().is_empty() || reference.len() > MAX_SOURCE_PATH_BYTES
        })
        || amendment.authorization_scope.trim().is_empty()
        || amendment.authorization_scope.len() > MAX_PIPELINE_INPUT_BYTES
        || amendment.authorization_provenance.trim().is_empty()
        || amendment.authorization_provenance.len() > MAX_PIPELINE_INPUT_BYTES
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

impl EscalatePipelineDelivery {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.run_id.is_nil()
            || self.run_revision < 1
            || self.phase_id.trim().is_empty()
            || self.reason.trim().is_empty()
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod source_amendment_tests {
    use super::*;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn proof_test_definition(version: &str) -> PipelineDefinitionSnapshot {
        let instruction = PipelineInstructionSnapshot {
            id: "instruction".into(),
            version: "1".into(),
            digest: "instruction-digest".into(),
            body: "instruction".into(),
            origin_refs: Vec::new(),
        };
        let phase = PipelinePhaseDefinition {
            id: "phase-1".into(),
            ordinal: 1,
            title: "Phase".into(),
            required: true,
            disposition_required: false,
            instructions: vec![instruction.clone()],
            skills: Vec::new(),
            resources: Vec::new(),
            required_artifacts: Vec::new(),
            validator_contracts: Vec::new(),
            followup_contracts: Vec::new(),
            required_fields: Vec::new(),
            allowed_verdicts: Vec::new(),
            required_dispositions: Vec::new(),
            allowed_dispositions: Vec::new(),
            output_constraints: Vec::new(),
            verdict_routes: Vec::new(),
            allowed_backward_to: Vec::new(),
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "contract".into(),
        };
        PipelineDefinitionSnapshot {
            kind: PipelineKind::LightweightTddDevelopment,
            version: version.into(),
            digest: "definition-digest".into(),
            overview: instruction,
            default_mode: PipelineDeliveryMode::Phasewise,
            allowed_modes: vec![PipelineDeliveryMode::Phasewise],
            phases: vec![phase],
            completion_contract: "completion".into(),
            escalation_contract: "escalation".into(),
            forbidden_claims: Vec::new(),
        }
    }

    fn proof_test_completion() -> CompletePipelinePhase {
        CompletePipelinePhase {
            request_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            run_revision: 1,
            phase_id: "phase-1".into(),
            outcome: PipelinePhaseOutcome::Completed,
            transition: PipelineTransition::Continue,
            output: PipelinePhaseOutputDraft {
                body: "body".into(),
                producer_context_id: "ctx".into(),
                fields: BTreeMap::new(),
                verdict: None,
                dispositions: Vec::new(),
                skill_reads: Vec::new(),
                resource_reads: Vec::new(),
                artifacts: Vec::new(),
                evidence_artifacts: Vec::new(),
                validator_receipts: Vec::new(),
                followup_proposal: None,
                knowledge_publication: None,
                reviewer_context: None,
                reference: None,
            },
            consumed_outputs: vec![PipelineConsumedOutput {
                phase_id: "previous".into(),
                output_revision: 1,
                digest: "output-digest".into(),
            }],
            consumed_inputs: vec![PipelineConsumedInput {
                input_id: Uuid::new_v4(),
                sequence: 1,
                digest: "input-digest".into(),
            }],
            revisit_phase_id: None,
            escalation_target: None,
            terminal_result: None,
            publish_blocked_result: false,
            consumed_knowledge: None,
            research_checkpoint: None,
        }
    }

    fn request(body: String) -> RecordPipelineInput {
        RecordPipelineInput {
            request_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            run_revision: 2,
            phase_id: "slice-implementation-spec-synthesizer".to_owned(),
            input: "Direct source amendment authority.".to_owned(),
            source_amendment: Some(PipelineSourceAmendment {
                target_phase_id: "slice-component-decision-interrogator".to_owned(),
                predecessor: PipelineSourcePredecessor {
                    output_id: Uuid::new_v4(),
                    output_revision: 1,
                    output_digest: "a".repeat(64),
                    artifact_name: "requirements-ledger.json".to_owned(),
                    artifact_digest: "b".repeat(64),
                    source_path: "source.md".to_owned(),
                    source_digest: "c".repeat(64),
                },
                successor: PipelineSourceSuccessor {
                    path: "source.md".to_owned(),
                    artifact: PipelineSourceArtifactDraft {
                        name: "source.md".to_owned(),
                        media_type: "text/markdown".to_owned(),
                        body,
                        digest: "d67e2e944994496c8d8ec76eed0cf9f09679448d584b532bebf941852a37f5ed"
                            .to_owned(),
                        reference: None,
                    },
                },
                authorization_scope: "Amend the current Full Design source.".to_owned(),
                authorization_provenance: "Exact direct operator input.".to_owned(),
            }),
        }
    }

    #[test]
    fn source_amendment_accepts_one_bounded_hash_valid_artifact() {
        assert_eq!(request("changed".to_owned()).validate(), Ok(()));
    }

    #[test]
    fn source_amendment_rejects_empty_even_with_the_empty_sha256() {
        assert_eq!(
            request(String::new()).validate(),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn source_amendment_rejects_unsafe_and_untrimmed_paths() {
        for path in ["../source.md", "/source.md", " source.md", "source.md "] {
            let mut value = request("changed".to_owned());
            let amendment = value.source_amendment.as_mut().unwrap();
            amendment.successor.path = path.to_owned();
            amendment.successor.artifact.name = path.to_owned();
            assert_eq!(value.validate(), Err(Error::InvalidArguments), "{path}");
        }
    }

    #[test]
    fn source_amendment_rejects_oversized_body() {
        assert_eq!(
            request("x".repeat(MAX_PIPELINE_OUTPUT_BYTES + 1)).validate(),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn source_amendment_rejects_path_name_mismatch() {
        let mut path = request("changed".to_owned());
        path.source_amendment
            .as_mut()
            .unwrap()
            .successor
            .artifact
            .name = "other.md".to_owned();
        assert_eq!(path.validate(), Err(Error::InvalidArguments));
    }

    #[test]
    fn source_amendment_leaves_digest_integrity_to_application() {
        let mut digest = request("changed".to_owned());
        digest
            .source_amendment
            .as_mut()
            .unwrap()
            .successor
            .artifact
            .digest = "0".repeat(64);
        assert_eq!(digest.validate(), Ok(()));
    }

    #[test]
    fn missing_test_target_has_typed_refusal_predicate() {
        let phase = PipelinePhaseDefinition {
            id: "tdd".into(),
            ordinal: 1,
            title: "TDD".into(),
            required: true,
            disposition_required: false,
            instructions: vec![],
            skills: vec![],
            resources: vec![],
            required_artifacts: vec![],
            validator_contracts: vec![],
            followup_contracts: vec![],
            required_fields: vec!["selected_test_target".into()],
            allowed_verdicts: vec![],
            required_dispositions: vec![],
            allowed_dispositions: vec![],
            output_constraints: vec![],
            verdict_routes: vec![],
            allowed_backward_to: vec![],
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "target".into(),
        };
        let output = PipelinePhaseOutputDraft {
            body: "body".into(),
            producer_context_id: "ctx".into(),
            fields: BTreeMap::new(),
            verdict: None,
            dispositions: vec![],
            skill_reads: vec![],
            resource_reads: vec![],
            artifacts: vec![],
            evidence_artifacts: vec![],
            validator_receipts: vec![],
            followup_proposal: None,
            knowledge_publication: None,
            reviewer_context: None,
            reference: None,
        };
        assert!(missing_test_target(&phase, &output));
    }

    #[test]
    fn backend_proof_is_rejected_only_for_current_definitions_with_exact_field_paths() {
        let legacy = proof_test_completion();
        assert_eq!(legacy.validate(&proof_test_definition("0.6.0")), Ok(()));

        let definition = proof_test_definition("0.7.0-native.k1k5");
        for (field, path) in [
            ("consumed_outputs", "arguments.params.consumed_outputs"),
            ("consumed_inputs", "arguments.params.consumed_inputs"),
        ] {
            let mut request = proof_test_completion();
            if field == "consumed_outputs" {
                request.consumed_inputs.clear();
            } else {
                request.consumed_outputs.clear();
            }
            let error = request.validate(&definition).unwrap_err();
            let refusal = error.refusal().expect("typed backend proof refusal");
            assert_eq!(error.code(), "BACKEND_DERIVED_PROOF_REQUIRED");
            assert_eq!(refusal.code, RefusalCode::BackendDerivedProofRequired);
            assert_eq!(refusal.rule.as_deref(), Some("WP3-PROOF-01"));
            assert_eq!(refusal.path.as_deref(), Some(path));
            assert_eq!(
                refusal.next_action.as_deref(),
                Some("omit_agent_supplied_proof")
            );
            assert_eq!(
                refusal.expected.as_deref(),
                Some("omitted; backend derives the proof")
            );
        }
    }

    #[test]
    fn legacy_resource_read_digest_mismatch_is_a_precise_invalid_output_refusal() {
        let mut definition = proof_test_definition("0.6.0");
        definition.phases[0]
            .resources
            .push(PipelineInstructionSnapshot {
                id: "test-resource".into(),
                version: "1".into(),
                digest: "expected-digest".into(),
                body: "resource".into(),
                origin_refs: Vec::new(),
            });
        let mut request = proof_test_completion();
        request
            .output
            .resource_reads
            .push(PipelineSkillReadReceipt {
                instruction_id: "test-resource".into(),
                version: "1".into(),
                digest: "expected-digest".into(),
            });
        assert_eq!(request.validate(&definition), Ok(()));

        request.output.resource_reads[0].digest = "substituted-digest".into();
        let error = request.validate(&definition).unwrap_err();
        let refusal = error.refusal().expect("typed resource-read refusal");
        assert_eq!(error.code(), "INVALID_OUTPUT");
        assert_eq!(refusal.code, RefusalCode::InvalidOutput);
        assert_eq!(refusal.rule.as_deref(), Some("WP6-RESOURCE-READ-01"));
        assert_eq!(
            refusal.path.as_deref(),
            Some("arguments.params.output.resource_reads")
        );
        assert_eq!(
            refusal.expected.as_deref(),
            Some(
                "exact pinned phase resource read receipts: [test-resource@1 digest=expected-digest]"
            )
        );
        assert_eq!(
            refusal.actual.as_deref(),
            Some(
                "submitted 1 receipt(s) (1 unique, 0 duplicate): [test-resource@1 digest=substituted-digest]"
            )
        );
        assert_eq!(
            refusal.next_action.as_deref(),
            Some("supply_exact_phase_resource_reads")
        );
        assert_eq!(
            refusal.required.as_deref(),
            Some("exact_phase_resource_reads")
        );
    }
}
