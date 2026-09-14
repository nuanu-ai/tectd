use crate::*;
use std::collections::BTreeSet;

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.as_bytes().contains(&0)
}

fn bounded(value: &str, max: usize) -> bool {
    value.len() <= max && !value.as_bytes().contains(&0)
}

fn iri(value: &str) -> bool {
    text(value, 4096)
        && (value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("urn:"))
}

fn list(values: &[String], max_items: usize, max_bytes: usize) -> bool {
    values.len() <= max_items
        && values.iter().all(|value| text(value, max_bytes))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn iris(values: &[String]) -> bool {
    values.len() <= DK2_MAX_LIST_ITEMS
        && values.iter().all(|value| iri(value))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn evidence_refs(values: &[u32], source_count: usize) -> bool {
    !values.is_empty()
        && values.len() <= DK2_MAX_LIST_ITEMS
        && values.iter().all(|index| (*index as usize) < source_count)
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

fn qualified_evidence(source: &KnowledgeSourceRef, expected: KnowledgeEvidenceKind) -> bool {
    match source {
        KnowledgeSourceRef::Snapshot { snapshot } => {
            snapshot.evidence_kind == expected
                && snapshot
                    .observed_at
                    .as_deref()
                    .is_some_and(|value| crate::knowledge_time::parse_rfc3339(value).is_some())
        }
        KnowledgeSourceRef::PipelineOutput { output } => {
            output.evidence_kind == expected
                && text(&output.evidence_scope, 4096)
                && output
                    .observed_at
                    .as_deref()
                    .is_some_and(|value| crate::knowledge_time::parse_rfc3339(value).is_some())
        }
    }
}

impl KnowledgeSourceRef {
    pub fn validate(&self) -> Result<()> {
        let valid =
            match self {
                Self::Snapshot { snapshot } => {
                    text(&snapshot.title, 1024)
                        && iri(&snapshot.uri)
                        && text(&snapshot.text, DK2_MAX_SOURCE_BYTES)
                        && snapshot.observed_at.as_deref().is_none_or(|value| {
                            crate::knowledge_time::parse_rfc3339(value).is_some()
                        })
                }
                Self::PipelineOutput { output } => {
                    !output.run_id.is_nil()
                        && !output.output_id.is_nil()
                        && text(&output.digest, 256)
                        && text(&output.evidence_scope, 4096)
                        && output.observed_at.as_deref().is_none_or(|value| {
                            crate::knowledge_time::parse_rfc3339(value).is_some()
                        })
                        && output.artifact.as_ref().is_none_or(|artifact| {
                            text(&artifact.name, 1024) && text(&artifact.digest, 256)
                        })
                }
            };
        valid.then_some(()).ok_or(Error::InvalidArguments)
    }
}

impl KnowledgeDocumentBinding {
    pub fn validate(&self) -> Result<()> {
        let target_valid = match &self.target {
            KnowledgeBindingTarget::Workspace => true,
            KnowledgeBindingTarget::Program { program_id } => !program_id.is_nil(),
            KnowledgeBindingTarget::Scope { scope_id } => !scope_id.is_nil(),
            KnowledgeBindingTarget::Slice { scope_id, slice_id } => {
                !scope_id.is_nil() && !slice_id.is_nil()
            }
            KnowledgeBindingTarget::SlicePhase {
                scope_id,
                slice_id,
                phase_id,
            } => !scope_id.is_nil() && !slice_id.is_nil() && text(phase_id, 256),
        };
        let version_valid = match self.version_resolution {
            KnowledgeBindingVersion::CurrentAccepted => true,
            KnowledgeBindingVersion::PinnedRevision { revision } => revision >= 1,
        };
        (target_valid && version_valid)
            .then_some(())
            .ok_or(Error::InvalidArguments)
    }
}

fn required_profile(kind: KnowledgeKind) -> Option<KnowledgeProfileId> {
    match kind {
        KnowledgeKind::Procedure => Some(KnowledgeProfileId::Runbook),
        KnowledgeKind::Protocol => Some(KnowledgeProfileId::Protocol),
        KnowledgeKind::Infrastructure => Some(KnowledgeProfileId::Devops),
        KnowledgeKind::OperatingModel => Some(KnowledgeProfileId::Operations),
        KnowledgeKind::ProductResearch => Some(KnowledgeProfileId::ProductResearch),
        KnowledgeKind::Security => Some(KnowledgeProfileId::Security),
        _ => None,
    }
}

impl KnowledgeDocumentDraft {
    pub fn validate(&self) -> Result<()> {
        let valid_from = self
            .valid_from
            .as_deref()
            .map(crate::knowledge_time::parse_rfc3339);
        let valid_until = self
            .valid_until
            .as_deref()
            .map(crate::knowledge_time::parse_rfc3339);
        if !text(&self.title, 1024)
            || !text(&self.canonical_text, DK2_MAX_SOURCE_BYTES)
            || self.target_iris.is_empty()
            || !iris(&self.target_iris)
            || !list(&self.conditions, DK2_MAX_LIST_ITEMS, 4096)
            || !list(&self.exceptions, DK2_MAX_LIST_ITEMS, 4096)
            || self.sources.is_empty()
            || self.sources.len() > DK2_MAX_LIST_ITEMS
            || self.bindings.is_empty()
            || self.bindings.len() > DK2_MAX_LIST_ITEMS
            || !text(&self.owner_ref, 1024)
            || !text(&self.authority_basis, 4096)
            || valid_from.is_some_and(|value| value.is_none())
            || valid_until.is_some_and(|value| value.is_none())
            || matches!((valid_from.flatten(), valid_until.flatten()), (Some(from), Some(until)) if from > until)
            || self
                .review_due_at
                .as_deref()
                .is_some_and(|value| crate::knowledge_time::parse_rfc3339(value).is_none())
            || self.profiles.iter().collect::<BTreeSet<_>>().len() != self.profiles.len()
            || self.profiles.windows(2).any(|pair| pair[0] >= pair[1])
            || !self.profiles.contains(&KnowledgeProfileId::General)
            || required_profile(self.knowledge_kind)
                .is_some_and(|profile| !self.profiles.contains(&profile))
            || (self.knowledge_kind == KnowledgeKind::Hypothesis
                && (self.epistemic_state != KnowledgeEpistemicState::Hypothesis
                    || self.sections.constraint.is_some()))
            || (self.knowledge_kind == KnowledgeKind::Constraint
                && self.epistemic_state != KnowledgeEpistemicState::Normative)
        {
            return Err(Error::InvalidArguments);
        }
        for source in &self.sources {
            source.validate()?;
        }
        for binding in &self.bindings {
            binding.validate()?;
        }
        self.validate_sections()?;
        if serde_json::to_vec(self)
            .map_err(|_| Error::InvalidArguments)?
            .len()
            > DK2_MAX_DOCUMENT_BYTES
        {
            return Err(Error::CapacityExceeded);
        }
        Ok(())
    }

    fn validate_sections(&self) -> Result<()> {
        let s = &self.sections;
        if self.knowledge_kind == KnowledgeKind::Constraint && s.constraint.is_none()
            || matches!(
                self.knowledge_kind,
                KnowledgeKind::Claim | KnowledgeKind::Decision | KnowledgeKind::Hypothesis
            ) && s.general.is_none()
            || self.profiles.contains(&KnowledgeProfileId::Runbook) && s.runbook.is_none()
            || self.profiles.contains(&KnowledgeProfileId::Protocol) && s.protocol.is_none()
            || self.profiles.contains(&KnowledgeProfileId::Devops) && s.devops.is_none()
            || self.profiles.contains(&KnowledgeProfileId::Operations) && s.operations.is_none()
            || self.profiles.contains(&KnowledgeProfileId::ProductResearch)
                && s.product_research.is_none()
            || self.profiles.contains(&KnowledgeProfileId::Security) && s.security.is_none()
            || s.runbook.is_some() && !self.profiles.contains(&KnowledgeProfileId::Runbook)
            || s.constraint.is_some() && self.knowledge_kind != KnowledgeKind::Constraint
            || s.protocol.is_some() && !self.profiles.contains(&KnowledgeProfileId::Protocol)
            || s.devops.is_some() && !self.profiles.contains(&KnowledgeProfileId::Devops)
            || s.operations.is_some() && !self.profiles.contains(&KnowledgeProfileId::Operations)
            || s.product_research.is_some()
                && !self.profiles.contains(&KnowledgeProfileId::ProductResearch)
            || s.security.is_some() && !self.profiles.contains(&KnowledgeProfileId::Security)
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(constraint) = &s.constraint
            && (!text(&constraint.action, 4096) || !iri(&constraint.target_iri))
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(general) = &s.general
            && (!text(&general.statement, DK2_MAX_SOURCE_BYTES)
                || !text(&general.evidence_scope, 4096)
                || !list(&general.assumptions, DK2_MAX_LIST_ITEMS, 4096)
                || !bounded(&general.rationale, DK2_MAX_SOURCE_BYTES)
                || !list(&general.alternatives, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&general.negative_limits, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&general.unknown_limits, DK2_MAX_LIST_ITEMS, 4096))
        {
            return Err(Error::InvalidArguments);
        }
        if self.knowledge_kind == KnowledgeKind::Decision
            && s.general.as_ref().is_none_or(|general| {
                !text(&general.rationale, DK2_MAX_SOURCE_BYTES) || general.alternatives.is_empty()
            })
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(runbook) = &s.runbook
            && (!text(&runbook.purpose_and_fit, 4096)
                || !iris(&runbook.target_environment_iris)
                || runbook.target_environment_iris.is_empty()
                || !text(&runbook.required_authority, 4096)
                || !text(&runbook.failure_and_recovery, DK2_MAX_SOURCE_BYTES)
                || runbook.parameters.len() > DK2_MAX_LIST_ITEMS
                || runbook
                    .parameters
                    .iter()
                    .map(|parameter| &parameter.name)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != runbook.parameters.len()
                || runbook.parameters.iter().any(|parameter| {
                    !text(&parameter.name, 256) || !text(&parameter.description, 4096)
                })
                || !list(&runbook.prerequisites, DK2_MAX_LIST_ITEMS, 4096)
                || !iris(&runbook.dependency_iris)
                || runbook.proof_evidence_refs.len() > DK2_MAX_LIST_ITEMS
                || runbook.steps.is_empty()
                || runbook.steps.len() > DK2_MAX_LIST_ITEMS
                || runbook.steps.iter().enumerate().any(|(index, step)| {
                    step.ordinal != u32::try_from(index + 1).unwrap_or(u32::MAX)
                        || !text(&step.action, 4096)
                        || !text(&step.expected_result, 4096)
                        || !text(&step.verification, 4096)
                })
                || runbook.proof_status == KnowledgeProofStatus::StaticVerified
                    && (!evidence_refs(&runbook.proof_evidence_refs, self.sources.len())
                        || !runbook.proof_evidence_refs.iter().all(|index| {
                            qualified_evidence(
                                &self.sources[*index as usize],
                                KnowledgeEvidenceKind::StaticVerification,
                            )
                        }))
                || runbook.proof_status == KnowledgeProofStatus::RuntimeVerified
                    && (!evidence_refs(&runbook.proof_evidence_refs, self.sources.len())
                        || !runbook.proof_evidence_refs.iter().all(|index| {
                            qualified_evidence(
                                &self.sources[*index as usize],
                                KnowledgeEvidenceKind::RuntimeVerification,
                            )
                        })))
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(protocol) = &s.protocol
            && (!iri(&protocol.specification_uri)
                || !text(&protocol.specification_version, 256)
                || protocol.provider_scope.is_empty()
                || protocol.network_scope.is_empty()
                || !list(&protocol.provider_scope, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&protocol.network_scope, DK2_MAX_LIST_ITEMS, 4096)
                || protocol.assertions.is_empty()
                || protocol.assertions.len() > DK2_MAX_LIST_ITEMS
                || protocol.assertions.iter().any(|assertion| {
                    !text(&assertion.statement, DK2_MAX_SOURCE_BYTES)
                        || !evidence_refs(&assertion.evidence_refs, self.sources.len())
                })
                || !list(&protocol.capabilities, DK2_MAX_LIST_ITEMS, 4096)
                || !list(
                    &protocol.compatibility_constraints,
                    DK2_MAX_LIST_ITEMS,
                    4096,
                )
                || !list(
                    &protocol.negative_states_and_quirks,
                    DK2_MAX_LIST_ITEMS,
                    4096,
                )
                || !text(&protocol.observation_bounds, 4096))
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(devops) = &s.devops
            && (devops.asset_iris.is_empty()
                || devops.environment_iris.is_empty()
                || !iris(&devops.asset_iris)
                || !iris(&devops.environment_iris)
                || devops.ownership.is_empty()
                || !list(&devops.ownership, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&devops.topology_links, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&devops.configuration_refs, DK2_MAX_LIST_ITEMS, 4096)
                || devops.observations.is_empty()
                || devops.observations.len() > DK2_MAX_LIST_ITEMS
                || devops.observations.iter().any(|observation| {
                    crate::knowledge_time::parse_rfc3339(&observation.observed_at).is_none()
                        || !text(&observation.status, 4096)
                        || !list(&observation.limits, DK2_MAX_LIST_ITEMS, 4096)
                        || !evidence_refs(&observation.evidence_refs, self.sources.len())
                })
                || devops.deployment_surfaces.is_empty()
                || !list(&devops.deployment_surfaces, DK2_MAX_LIST_ITEMS, 4096)
                || !text(&devops.configuration_custody, 4096))
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(operations) = &s.operations
            && (!text(&operations.operating_purpose, DK2_MAX_SOURCE_BYTES)
                || operations.roles.is_empty()
                || operations.roles.len() > DK2_MAX_LIST_ITEMS
                || operations.roles.iter().any(|role| {
                    !text(&role.role, 1024)
                        || role.responsibilities.is_empty()
                        || !list(&role.responsibilities, DK2_MAX_LIST_ITEMS, 4096)
                })
                || !text(&operations.cadence, 4096)
                || !list(&operations.handoffs, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&operations.escalation, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&operations.status_semantics, DK2_MAX_LIST_ITEMS, 4096)
                || operations.metrics.len() > DK2_MAX_LIST_ITEMS
                || operations.metrics.iter().any(|metric| {
                    !text(&metric.name, 1024)
                        || !text(&metric.meaning, 4096)
                        || !text(&metric.objective, 4096)
                        || !text(&metric.signal_source, 4096)
                })
                || !list(&operations.signal_sources, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&operations.exceptions, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&operations.ownership_gaps, DK2_MAX_LIST_ITEMS, 4096))
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(research) = &s.product_research
            && (!text(&research.question, DK2_MAX_SOURCE_BYTES)
                || research.evidence_map.is_empty()
                || research.evidence_map.len() > DK2_MAX_LIST_ITEMS
                || research.evidence_map.iter().any(|evidence| {
                    !text(&evidence.claim, DK2_MAX_SOURCE_BYTES)
                        || !evidence_refs(&evidence.source_refs, self.sources.len())
                })
                || !list(&research.assumptions, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&research.segments, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&research.alternatives, DK2_MAX_LIST_ITEMS, 4096)
                || research.conclusions_and_decisions.is_empty()
                || !list(
                    &research.conclusions_and_decisions,
                    DK2_MAX_LIST_ITEMS,
                    DK2_MAX_SOURCE_BYTES,
                )
                || !list(&research.observation_limits, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&research.negative_evidence, DK2_MAX_LIST_ITEMS, 4096))
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(security) = &s.security
            && (security.asset_iris.is_empty()
                || !iris(&security.asset_iris)
                || security.trust_boundaries.is_empty()
                || security.threats.is_empty()
                || security.controls.is_empty()
                || !list(&security.trust_boundaries, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&security.threats, DK2_MAX_LIST_ITEMS, 4096)
                || !list(&security.controls, DK2_MAX_LIST_ITEMS, 4096)
                || !evidence_refs(&security.evidence_refs, self.sources.len())
                || !text(&security.verification_status, 4096)
                || !text(&security.applicable_authority, 4096)
                || !text(&security.finding_state, 4096)
                || !list(&security.exceptions, DK2_MAX_LIST_ITEMS, 4096)
                || security.remediation_proof_refs.len() > DK2_MAX_LIST_ITEMS
                || !security.remediation_proof_refs.is_empty()
                    && !evidence_refs(&security.remediation_proof_refs, self.sources.len())
                || matches!(
                    security.sensitivity,
                    KnowledgeSensitivity::Sensitive | KnowledgeSensitivity::Restricted
                ) && self.access_scope != KnowledgeAccessScope::OwnersOnly)
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}
