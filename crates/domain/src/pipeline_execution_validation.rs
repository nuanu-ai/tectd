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

mod completion;
mod completion_helpers;
use completion_helpers::*;

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
mod source_amendment_tests;
