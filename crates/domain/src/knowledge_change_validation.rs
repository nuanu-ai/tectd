use crate::*;
use std::collections::BTreeSet;

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.as_bytes().contains(&0)
}

fn contract_ref(value: &KnowledgeContractRef) -> bool {
    text(&value.id, 256)
        && text(&value.version, 128)
        && text(&value.digest, 256)
        && text(&value.source_ref, 4096)
}

fn instruction(value: &PipelineInstructionSnapshot) -> bool {
    text(&value.id, 256)
        && text(&value.version, 128)
        && text(&value.digest, 256)
        && text(&value.body, DK2_MAX_SOURCE_BYTES)
        && !value.origin_refs.is_empty()
        && value.origin_refs.iter().all(|source| text(source, 4096))
}

fn expected_phase_contract(
    phase: KnowledgeChangePhaseId,
) -> (KnowledgePhaseExecutor, KnowledgePhaseOutputKind) {
    match phase {
        KnowledgeChangePhaseId::KcIntake => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::ChangeIntent,
        ),
        KnowledgeChangePhaseId::KcResolveBaseline => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::BaselineManifest,
        ),
        KnowledgeChangePhaseId::KcQualifyPlan => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::BranchPlan,
        ),
        KnowledgeChangePhaseId::KcQualifyEvidence => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::EvidenceManifest,
        ),
        KnowledgeChangePhaseId::KcPrepareChange => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::ProposedChangeset,
        ),
        KnowledgeChangePhaseId::KcDomainChecks => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::ObligationReceipts,
        ),
        KnowledgeChangePhaseId::KcImpactPlan => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::ImpactPlan,
        ),
        KnowledgeChangePhaseId::KcReviewReconcile => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::ReviewReceipt,
        ),
        KnowledgeChangePhaseId::KcPublicationGate => (
            KnowledgePhaseExecutor::Backend,
            KnowledgePhaseOutputKind::ReadyToCommit,
        ),
        KnowledgeChangePhaseId::KcCommit => (
            KnowledgePhaseExecutor::Publisher,
            KnowledgePhaseOutputKind::PublisherReceipt,
        ),
        KnowledgeChangePhaseId::KcSettleEffects => (
            KnowledgePhaseExecutor::Backend,
            KnowledgePhaseOutputKind::EffectsReport,
        ),
        KnowledgeChangePhaseId::KcResultHandoff => (
            KnowledgePhaseExecutor::Agent,
            KnowledgePhaseOutputKind::ChangeResult,
        ),
    }
}

impl BeginKnowledgeChange {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || !text(&self.intent, DK2_MAX_SOURCE_BYTES)
            || !text(&self.desired_outcome, 4096)
            || self.sources.len() > DK2_MAX_LIST_ITEMS
            || self.operation_hints.is_empty()
            || self.operation_hints.len() > DK2_MAX_OPERATIONS
        {
            return Err(Error::InvalidArguments);
        }
        if let KnowledgeChangeOwner::PromotionSlice {
            scope_id,
            slice_id,
            slice_revision,
        } = self.owner
            && (scope_id.is_nil() || slice_id.is_nil() || slice_revision < 1)
        {
            return Err(Error::InvalidArguments);
        }
        for source in &self.sources {
            source.validate()?;
        }
        let mut labels = BTreeSet::new();
        for hint in &self.operation_hints {
            if !labels.insert(hint.client_label.as_str())
                || !text(&hint.client_label, 128)
                || !text(&hint.reason, 4096)
                || !text(&hint.authority_basis, 4096)
                || hint.depends_on_labels.iter().any(|label| !text(label, 128))
            {
                return Err(Error::InvalidArguments);
            }
            match hint.operation {
                KnowledgeLifecycleOperation::Create
                    if hint.unit_id.is_none()
                        && hint.expected_revision.is_none()
                        && hint.expected_lifecycle.is_none() => {}
                KnowledgeLifecycleOperation::Create => return Err(Error::InvalidArguments),
                _ if hint.unit_id.is_some_and(|id| !id.is_nil())
                    && hint.expected_revision.is_some_and(|revision| revision >= 1)
                    && hint.expected_lifecycle.is_some() => {}
                _ => return Err(Error::InvalidArguments),
            }
        }
        if self.operation_hints.iter().any(|hint| {
            hint.depends_on_labels
                .iter()
                .any(|label| label == &hint.client_label || !labels.contains(label.as_str()))
        }) {
            return Err(Error::InvalidArguments);
        }
        let erases = self
            .operation_hints
            .iter()
            .any(|hint| hint.operation == KnowledgeLifecycleOperation::Erase);
        if erases == (self.completion.erasure == KnowledgeErasureRequirement::NotRequired) {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

impl KnowledgeProfileRegistry {
    pub fn validate(&self) -> Result<()> {
        if !text(&self.version, 128)
            || !text(&self.digest, 256)
            || self.profiles.len() != 7
            || self
                .profiles
                .windows(2)
                .any(|pair| pair[0].profile_id >= pair[1].profile_id)
            || self
                .profiles
                .iter()
                .map(|p| p.profile_id)
                .collect::<BTreeSet<_>>()
                .len()
                != 7
        {
            return Err(Error::InvalidArguments);
        }
        for profile in &self.profiles {
            if !text(&profile.version, 128)
                || !text(&profile.digest, 256)
                || profile.operations.len() != 6
                || profile.operations.iter().collect::<BTreeSet<_>>().len() != 6
                || profile.operations.windows(2).any(|pair| pair[0] >= pair[1])
                || profile.profile_id != KnowledgeProfileId::General
                    && !profile.inherits.contains(&KnowledgeProfileId::General)
                || profile.obligations.is_empty()
                || profile
                    .obligations
                    .windows(2)
                    .any(|pair| (pair[0].phase_id, &pair[0].id) >= (pair[1].phase_id, &pair[1].id))
                || profile.shape_refs.iter().any(|value| !contract_ref(value))
                || profile.methods.iter().any(|value| !instruction(value))
                || !profile.methods.iter().any(|method| {
                    method.id == profile.profile_id.method_id() && method.version == profile.version
                })
            {
                return Err(Error::InvalidArguments);
            }
            for obligation in &profile.obligations {
                let applicability_valid = match &obligation.applicability {
                    KnowledgeObligationApplicability::Operations { operations } => {
                        !operations.is_empty()
                            && operations.iter().collect::<BTreeSet<_>>().len() == operations.len()
                            && !operations.windows(2).any(|pair| pair[0] >= pair[1])
                    }
                    KnowledgeObligationApplicability::DeclaredCondition { condition_id } => {
                        text(condition_id, 256)
                    }
                    _ => true,
                };
                if !text(&obligation.id, 256)
                    || !text(&obligation.requirement, 4096)
                    || !obligation.required
                    || !applicability_valid
                    || obligation.method_refs.is_empty()
                    || !obligation.method_refs.iter().all(contract_ref)
                    || obligation.method_refs.iter().any(|method| {
                        !profile.methods.iter().any(|candidate| {
                            candidate.id == method.id
                                && candidate.version == method.version
                                && candidate.digest == method.digest
                                && candidate.origin_refs.contains(&method.source_ref)
                        })
                    })
                    || !obligation.shape_refs.iter().all(contract_ref)
                    || !contract_ref(&obligation.reuse_rule_ref)
                {
                    return Err(Error::InvalidArguments);
                }
            }
        }
        Ok(())
    }
}

impl KnowledgeChangeDefinition {
    pub fn validate(&self) -> Result<()> {
        if !text(&self.version, 128)
            || !text(&self.digest, 256)
            || !text(&self.registry_version, 128)
            || !text(&self.registry_digest, 256)
            || !self.allowed_modes.contains(&self.default_mode)
            || self.phases.len() != KnowledgeChangePhaseId::ALL.len()
            || !contract_ref(&self.completion_contract_ref)
            || !contract_ref(&self.escalation_contract_ref)
            || !instruction(&self.overview)
        {
            return Err(Error::InvalidArguments);
        }
        for (index, phase) in self.phases.iter().enumerate() {
            let expected = KnowledgeChangePhaseId::ALL[index];
            let (expected_executor, expected_output) = expected_phase_contract(expected);
            if phase.id != expected
                || phase.ordinal != expected.ordinal()
                || !text(&phase.title, 256)
                || !contract_ref(&phase.output_contract_ref)
                || phase.executor != expected_executor
                || phase.output_kind != expected_output
                || phase.executor == KnowledgePhaseExecutor::Agent && phase.methods.is_empty()
                || phase.executor != KnowledgePhaseExecutor::Agent && !phase.methods.is_empty()
                || phase.methods.iter().any(|method| !instruction(method))
                || expected.method_id().is_some_and(|method_id| {
                    !phase
                        .methods
                        .iter()
                        .any(|method| method.id == method_id && method.version == self.version)
                })
            {
                return Err(Error::InvalidArguments);
            }
        }
        Ok(())
    }
}

impl KnowledgeBranchPlan {
    pub fn validate(&self) -> Result<()> {
        if self.revision < 1
            || !text(&self.digest, 256)
            || !text(&self.definition_version, 128)
            || !text(&self.definition_digest, 256)
            || !text(&self.registry_version, 128)
            || !text(&self.registry_digest, 256)
            || self.operation_ids.is_empty()
            || self.operation_ids.len() > DK2_MAX_OPERATIONS
            || self.operation_ids.iter().any(uuid::Uuid::is_nil)
            || self.operation_ids.iter().collect::<BTreeSet<_>>().len() != self.operation_ids.len()
            || !self.profiles.contains(&KnowledgeProfileId::General)
            || self.profiles.iter().collect::<BTreeSet<_>>().len() != self.profiles.len()
            || self.obligations.is_empty()
            || self.obligations.iter().any(|obligation| {
                obligation.operation_id.is_nil()
                    || !self.operation_ids.contains(&obligation.operation_id)
                    || !text(&obligation.obligation_id, 256)
                    || !text(&obligation.requirement, 4096)
                    || !text(&obligation.profile_version, 128)
                    || !text(&obligation.profile_digest, 256)
                    || obligation
                        .method_refs
                        .iter()
                        .any(|value| !contract_ref(value))
                    || obligation
                        .shape_refs
                        .iter()
                        .any(|value| !contract_ref(value))
            })
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}
