use super::*;

mod aggregate;
mod tail;
use aggregate::first_aggregate_refusal;
use tail::validate_completion_tail;

impl CompletePipelinePhase {
    pub fn validate(&self, definition: &PipelineDefinitionSnapshot) -> Result<()> {
        if definition.version.starts_with("0.7") {
            reject_agent_supplied_proof(self)?;
        }
        if self.request_id.is_nil() {
            return Err(completion_refusal(
                RefusalCode::InputSchemaInvalid,
                "WP6-COMPLETE-REQUEST-01",
                "arguments.params.request_id",
                "non-nil request UUID",
                "nil UUID",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if self.run_id.is_nil() {
            return Err(completion_refusal(
                RefusalCode::InputSchemaInvalid,
                "WP6-COMPLETE-REQUEST-02",
                "arguments.params.run_id",
                "non-nil run UUID",
                "nil UUID",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if self.run_revision < 1 {
            return Err(completion_refusal(
                RefusalCode::InputSchemaInvalid,
                "WP6-COMPLETE-REQUEST-03",
                "arguments.params.run_revision",
                "positive revision",
                self.run_revision.to_string(),
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if !definition.version.starts_with("0.7") && self.output.body.trim().is_empty() {
            return Err(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-COMPLETE-OUTPUT-01",
                "arguments.params.output.body",
                "non-empty legacy output body",
                "empty",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if self.output.producer_context_id.trim().is_empty() {
            return Err(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-COMPLETE-OUTPUT-02",
                "arguments.params.output.producer_context_id",
                "non-empty producer context label",
                "empty",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if self.output.producer_context_id.len() > MAX_PIPELINE_CONTEXT_ID_BYTES {
            return Err(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-COMPLETE-OUTPUT-03",
                "arguments.params.output.producer_context_id",
                "producer label within the context byte limit",
                self.output.producer_context_id.len().to_string(),
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if self
            .output
            .reference
            .as_ref()
            .is_some_and(|v| v.trim().is_empty())
        {
            return Err(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-COMPLETE-OUTPUT-04",
                "arguments.params.output.reference",
                "non-empty reference when supplied",
                "empty",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        for (index, value) in self.consumed_outputs.iter().enumerate() {
            if value.phase_id.trim().is_empty() {
                return Err(completion_refusal(
                    RefusalCode::InputSchemaInvalid,
                    "WP6-COMPLETE-REQUEST-04-1",
                    format!("arguments.params.consumed_outputs[{index}].phase_id"),
                    "non-empty phase id",
                    "empty",
                    "correct_phase_completion",
                    "valid_phase_completion",
                ));
            }
            if value.output_revision < 1 {
                return Err(completion_refusal(
                    RefusalCode::InputSchemaInvalid,
                    "WP6-COMPLETE-REQUEST-04-2",
                    format!("arguments.params.consumed_outputs[{index}].output_revision"),
                    "positive output revision",
                    value.output_revision.to_string(),
                    "correct_phase_completion",
                    "valid_phase_completion",
                ));
            }
            if value.digest.trim().is_empty() {
                return Err(completion_refusal(
                    RefusalCode::InputSchemaInvalid,
                    "WP6-COMPLETE-REQUEST-04-3",
                    format!("arguments.params.consumed_outputs[{index}].digest"),
                    "non-empty output digest",
                    "empty",
                    "correct_phase_completion",
                    "valid_phase_completion",
                ));
            }
        }
        for (index, value) in self.consumed_inputs.iter().enumerate() {
            if value.input_id.is_nil() {
                return Err(completion_refusal(
                    RefusalCode::InputSchemaInvalid,
                    "WP6-COMPLETE-REQUEST-05-1",
                    format!("arguments.params.consumed_inputs[{index}].input_id"),
                    "non-nil input UUID",
                    "nil UUID",
                    "correct_phase_completion",
                    "valid_phase_completion",
                ));
            }
            if value.sequence < 1 {
                return Err(completion_refusal(
                    RefusalCode::InputSchemaInvalid,
                    "WP6-COMPLETE-REQUEST-05-2",
                    format!("arguments.params.consumed_inputs[{index}].sequence"),
                    "positive input sequence",
                    value.sequence.to_string(),
                    "correct_phase_completion",
                    "valid_phase_completion",
                ));
            }
            if value.digest.trim().is_empty() {
                return Err(completion_refusal(
                    RefusalCode::InputSchemaInvalid,
                    "WP6-COMPLETE-REQUEST-05-3",
                    format!("arguments.params.consumed_inputs[{index}].digest"),
                    "non-empty input digest",
                    "empty",
                    "correct_phase_completion",
                    "valid_phase_completion",
                ));
            }
        }
        let phase = definition
            .phases
            .iter()
            .find(|phase| phase.id == self.phase_id)
            .ok_or_else(|| {
                completion_refusal(
                    RefusalCode::InputSchemaInvalid,
                    "WP6-COMPLETE-REQUEST-06",
                    "arguments.params.phase_id",
                    "phase id present in the pinned definition",
                    "no matching phase",
                    "select_current_phase",
                    "current_phase_id",
                )
            })?;
        let creates_checkpoint = definition.kind == PipelineKind::DeepBrainstorming
            && phase.ordinal == 5
            && self.output.verdict.as_deref() == Some("waiting_research");
        if creates_checkpoint != self.research_checkpoint.is_some() {
            return Err(completion_refusal(
                RefusalCode::InputSchemaInvalid,
                "WP6-COMPLETE-REQUEST-07",
                "arguments.params.research_checkpoint",
                "checkpoint presence matching waiting_research verdict",
                format!(
                    "expected_presence={creates_checkpoint}; supplied_presence={}",
                    self.research_checkpoint.is_some()
                ),
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }

        if let Some(checkpoint) = &self.research_checkpoint {
            checkpoint.validate()?;
            if definition.kind != PipelineKind::DeepBrainstorming
                || phase.ordinal != 5
                || self.outcome != PipelinePhaseOutcome::WaitingInput
                || self.transition != PipelineTransition::Continue
                || self.output.verdict.as_deref() != Some("waiting_research")
            {
                return Err(completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-COMPLETE-OUTPUT-13",
                    "arguments.params.research_checkpoint",
                    "deep brainstorming phase 5 waiting-input continue waiting_research completion",
                    "checkpoint completion predicates differ",
                    "align_research_checkpoint_completion",
                    "valid_research_checkpoint_completion",
                ));
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
        let aggregate_refusal = first_aggregate_refusal(self, definition, phase);
        if let Some(error) = aggregate_refusal {
            if definition.version.starts_with("0.7") && missing_test_target(phase, &self.output) {
                return Err(completion_refusal(
                    RefusalCode::NoTestTarget,
                    "WP6-TEST-TARGET-01",
                    "arguments.params.output.fields.selected_test_target",
                    "a non-empty executable test target",
                    "missing or empty",
                    "select_test_target",
                    "selected_test_target",
                ));
            }
            return Err(error);
        }
        for constraint in &phase.output_constraints {
            if !output_constraint_satisfied(&self.output, constraint) {
                return Err(output_constraint_refusal(constraint, &self.output));
            }
        }
        validate_completion_constraints(self, definition, phase)?;
        validate_artifacts(phase, &self.output)?;
        validate_native_work_contract_output(self, definition, phase)?;
        validate_completion_tail(self, definition, phase)
    }
}
