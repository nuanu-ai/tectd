use crate::{
    engineering_review::validate_completion_constraints, pipeline_artifacts::validate_artifacts,
    pipeline_constraints::output_constraint_satisfied,
    pipeline_followups::validate_followup_proposal, *,
};
use std::collections::BTreeSet;

use super::completion_helpers::{
    missing_test_target, output_constraint_refusal, phase_read_receipt_details,
    phase_read_receipt_refusal, reject_agent_supplied_proof, validate_terminal,
};

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
