use crate::{
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
        if self.request_id.is_nil()
            || self.scope_id.is_nil()
            || self.slice_id.is_nil()
            || self.slice_revision < 1
            || self.qualification_reason.trim().is_empty()
            || !definition
                .allowed_modes
                .contains(&self.delivery_mode.unwrap_or(definition.default_mode))
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
        if self.request_id.is_nil()
            || self.run_id.is_nil()
            || self.run_revision < 1
            || self.output.body.trim().is_empty()
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
        if phase.required_fields.iter().any(|key| {
            self.output
                .fields
                .get(key)
                .is_none_or(|v| v.trim().is_empty())
        }) || (!phase.allowed_verdicts.is_empty()
            && self
                .output
                .verdict
                .as_ref()
                .is_none_or(|v| !phase.allowed_verdicts.contains(v)))
            || phase
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
            return Err(Error::InvalidArguments);
        }
        for constraint in &phase.output_constraints {
            if !output_constraint_satisfied(&self.output, constraint) {
                return Err(Error::InvalidArguments);
            }
        }
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
            return Err(Error::InvalidArguments);
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
            return Err(Error::InvalidArguments);
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
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
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
