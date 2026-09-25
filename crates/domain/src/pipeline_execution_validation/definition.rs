use crate::{
    engineering_review::validate_definition_constraints,
    pipeline_artifacts::validate_artifact_definition,
    pipeline_constraints::validate_output_constraint,
    pipeline_followups::validate_followup_definitions, *,
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
