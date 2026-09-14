use crate::*;
use std::collections::BTreeSet;

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.as_bytes().contains(&0)
}

fn iri(value: &str) -> bool {
    text(value, 4096)
        && (value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("urn:"))
}

fn valid_fragment(value: &KnowledgeLifecycleFragmentQuery) -> bool {
    (1..=262_144).contains(&value.limit)
        && value
            .snapshot_digest
            .as_deref()
            .is_none_or(|digest| text(digest, 256))
        && (value.offset == 0 || value.snapshot_digest.is_some())
}

impl KnowledgeRevalidationDraft {
    pub fn validate(&self) -> Result<()> {
        if self.sources.is_empty()
            || self.sources.len() > DK2_MAX_LIST_ITEMS
            || !text(&self.evidence_basis, DK2_MAX_SOURCE_BYTES)
            || self
                .valid_until
                .as_deref()
                .is_some_and(|value| crate::knowledge_time::parse_rfc3339(value).is_none())
            || self
                .review_due_at
                .as_deref()
                .is_some_and(|value| crate::knowledge_time::parse_rfc3339(value).is_none())
        {
            return Err(Error::InvalidArguments);
        }
        self.sources
            .iter()
            .try_for_each(KnowledgeSourceRef::validate)
    }
}

impl KnowledgePlanQualification {
    pub fn validate(&self) -> Result<()> {
        if self.operations.is_empty()
            || self.operations.len() > DK2_MAX_OPERATIONS
            || self.operations.iter().any(|operation| {
                operation.operation_id.is_nil()
                    || !text(&operation.classification_basis, 4096)
                    || !operation.profiles.contains(&KnowledgeProfileId::General)
                    || operation.profiles.windows(2).any(|pair| pair[0] >= pair[1])
            })
            || self
                .operations
                .iter()
                .map(|operation| operation.operation_id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.operations.len()
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl KnowledgePlannedOperation {
    fn validate_with_binding_provenance(&self, resolved: bool) -> Result<()> {
        let bindings = self
            .document
            .as_ref()
            .map(|document| document.bindings.as_slice())
            .unwrap_or(self.replacement_bindings.as_slice());
        let slice_phase_indexes = bindings
            .iter()
            .enumerate()
            .filter_map(|(index, binding)| {
                matches!(binding.target, KnowledgeBindingTarget::SlicePhase { .. })
                    .then_some(index as u32)
            })
            .collect::<Vec<_>>();
        if self.operation_id.is_nil()
            || self.unit_id.is_nil()
            || !text(&self.client_label, 128)
            || !text(&self.reason, 4096)
            || !text(&self.authority_basis, 4096)
            || self.dependency_operation_ids.iter().any(uuid::Uuid::is_nil)
            || self
                .dependency_operation_ids
                .iter()
                .collect::<BTreeSet<_>>()
                .len()
                != self.dependency_operation_ids.len()
            || self.dependency_operation_ids.contains(&self.operation_id)
            || self.binding_pins.windows(2).any(|pair| pair[0].binding_index >= pair[1].binding_index)
            || if resolved {
                self.binding_pins
                    .iter()
                    .map(|pin| pin.binding_index)
                    .collect::<Vec<_>>()
                    != slice_phase_indexes
            } else {
                !self.binding_pins.is_empty()
            }
            || self.binding_pins.iter().any(|pin| {
                pin.definition_version.trim().is_empty()
                    || pin.definition_digest.trim().is_empty()
                    || pin.phase_id.trim().is_empty()
                    || bindings
                        .get(pin.binding_index as usize)
                        .is_none_or(|binding| !matches!(&binding.target,KnowledgeBindingTarget::SlicePhase{phase_id,..} if phase_id==&pin.phase_id))
            })
        {
            return Err(Error::InvalidArguments);
        }
        match self.operation {
            KnowledgeLifecycleOperation::Create
                if self.expected_revision.is_none()
                    && self.expected_lifecycle.is_none()
                    && self.document.is_some()
                    && self.revalidation.is_none()
                    && self.successor.is_none() => {}
            KnowledgeLifecycleOperation::Revise
                if self.expected_revision.is_some_and(|revision| revision >= 1)
                    && self.expected_lifecycle.is_some()
                    && self.document.is_some()
                    && self.revalidation.is_none()
                    && self.successor.is_none() => {}
            KnowledgeLifecycleOperation::Revalidate
                if self.expected_revision.is_some_and(|revision| revision >= 1)
                    && self.expected_lifecycle.is_some()
                    && self.document.is_none()
                    && self.revalidation.is_some()
                    && self.successor.is_none() => {}
            KnowledgeLifecycleOperation::Supersede
                if self.expected_revision.is_some_and(|revision| revision >= 1)
                    && self.expected_lifecycle.is_some()
                    && self.successor.is_some()
                    && self.revalidation.is_none()
                    && !self.replacement_bindings.is_empty() => {}
            KnowledgeLifecycleOperation::Retract | KnowledgeLifecycleOperation::Erase
                if self.expected_revision.is_some_and(|revision| revision >= 1)
                    && self.expected_lifecycle.is_some()
                    && self.document.is_none()
                    && self.revalidation.is_none()
                    && self.successor.is_none() => {}
            _ => return Err(Error::InvalidArguments),
        }
        if let Some(document) = &self.document {
            document.validate()?;
        }
        if let Some(revalidation) = &self.revalidation {
            revalidation.validate()?;
        }
        if let Some(successor) = &self.successor {
            match (successor.unit_id, successor.operation_id) {
                (Some(unit_id), None) if !unit_id.is_nil() && unit_id != self.unit_id => {}
                (None, Some(operation_id))
                    if !operation_id.is_nil() && operation_id != self.operation_id => {}
                _ => return Err(Error::InvalidArguments),
            }
        }
        for binding in &self.replacement_bindings {
            binding.validate()?;
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_with_binding_provenance(true)
    }

    pub(crate) fn validate_before_binding_resolution(&self) -> Result<()> {
        self.validate_with_binding_provenance(false)
    }
}

impl KnowledgeProposedChangeset {
    pub fn validate(&self) -> Result<()> {
        self.validate_with_binding_provenance(true)
    }

    pub(crate) fn validate_before_binding_resolution(&self) -> Result<()> {
        self.validate_with_binding_provenance(false)
    }

    fn validate_with_binding_provenance(&self, resolved: bool) -> Result<()> {
        if self.revision < 1
            || self.operations.is_empty()
            || self.operations.len() > DK2_MAX_OPERATIONS
            || !text(&self.semantic_diff, DK2_MAX_SOURCE_BYTES)
            || !text(&self.evidence_digest, 256)
            || !self.digest.is_empty() && !text(&self.digest, 256)
        {
            return Err(Error::InvalidArguments);
        }
        let mut operation_ids = BTreeSet::new();
        let mut labels = BTreeSet::new();
        for operation in &self.operations {
            if resolved {
                operation.validate()?;
            } else {
                operation.validate_before_binding_resolution()?;
            }
            if !operation_ids.insert(operation.operation_id)
                || !labels.insert(operation.client_label.as_str())
            {
                return Err(Error::InvalidArguments);
            }
        }
        if self.operations.iter().any(|operation| {
            operation
                .dependency_operation_ids
                .iter()
                .any(|id| !operation_ids.contains(id))
                || operation.successor.as_ref().is_some_and(|successor| {
                    successor
                        .operation_id
                        .is_some_and(|id| !operation_ids.contains(&id))
                })
        }) {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

impl KnowledgeLifecycleQuery {
    pub fn validate(&self) -> Result<()> {
        if self
            .fragment
            .as_ref()
            .is_some_and(|value| !valid_fragment(value))
        {
            return Err(Error::InvalidArguments);
        }
        match (
            self.change_id,
            self.view,
            self.output_id,
            self.digest.as_deref(),
        ) {
            (None, KnowledgeLifecycleView::Current, None, None) => Ok(()),
            (
                Some(id),
                KnowledgeLifecycleView::Current | KnowledgeLifecycleView::History,
                None,
                None,
            ) if !id.is_nil() => Ok(()),
            (Some(id), KnowledgeLifecycleView::Output, Some(output), Some(digest))
                if !id.is_nil() && !output.is_nil() && text(digest, 256) =>
            {
                Ok(())
            }
            _ => Err(Error::InvalidArguments),
        }
    }
}

impl KnowledgeUnitQuery {
    pub fn validate(&self) -> Result<()> {
        if self.unit_id.is_nil()
            || self.revision.is_some_and(|revision| revision < 1)
            || self
                .fragment
                .as_ref()
                .is_some_and(|value| !valid_fragment(value))
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl CompleteKnowledgeChangePhase {
    pub fn validate(&self) -> Result<()> {
        self.validate_with_binding_provenance(true)
    }

    pub fn validate_before_binding_resolution(&self) -> Result<()> {
        self.validate_with_binding_provenance(false)
    }

    fn validate_with_binding_provenance(&self, resolved: bool) -> Result<()> {
        if self.request_id.is_nil()
            || self.change_id.is_nil()
            || self.run_id.is_nil()
            || self.run_revision < 1
            || self.phase_id == KnowledgeChangePhaseId::KcCommit
            || self.phase_id.agent_authored() != self.output.is_some()
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(output) = &self.output
            && (output.phase_id != self.phase_id
                || output.expected_run_revision != self.run_revision
                || !text(&output.body, DK2_MAX_SOURCE_BYTES)
                || !text(&output.verdict, 256)
                || output.method_reads.is_empty()
                || output.data.phase_id() != self.phase_id)
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(output) = &self.output {
            output.validate_bounded_with_binding_provenance(resolved)?;
        }
        Ok(())
    }
}

impl KnowledgeAgentPhaseData {
    pub const fn phase_id(&self) -> KnowledgeChangePhaseId {
        match self {
            Self::KcIntake(_) => KnowledgeChangePhaseId::KcIntake,
            Self::KcResolveBaseline(_) => KnowledgeChangePhaseId::KcResolveBaseline,
            Self::KcQualifyPlan(_) => KnowledgeChangePhaseId::KcQualifyPlan,
            Self::KcQualifyEvidence(_) => KnowledgeChangePhaseId::KcQualifyEvidence,
            Self::KcPrepareChange(_) => KnowledgeChangePhaseId::KcPrepareChange,
            Self::KcDomainChecks(_) => KnowledgeChangePhaseId::KcDomainChecks,
            Self::KcImpactPlan(_) => KnowledgeChangePhaseId::KcImpactPlan,
            Self::KcReviewReconcile(_) => KnowledgeChangePhaseId::KcReviewReconcile,
            Self::KcResultHandoff(_) => KnowledgeChangePhaseId::KcResultHandoff,
        }
    }
}

fn request_identity(
    request_id: uuid::Uuid,
    change_id: uuid::Uuid,
    run_id: uuid::Uuid,
    run_revision: i64,
) -> Result<()> {
    if request_id.is_nil() || change_id.is_nil() || run_id.is_nil() || run_revision < 1 {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

impl RecordKnowledgeChangeInput {
    pub fn validate(&self) -> Result<()> {
        request_identity(
            self.request_id,
            self.change_id,
            self.run_id,
            self.run_revision,
        )?;
        if !text(&self.reason, 4096) || !text(&self.input, DK2_MAX_SOURCE_BYTES) {
            Err(Error::InvalidArguments)
        } else if let Some(amendment) = &self.basis_amendment {
            if amendment.target_updates.len() > DK2_MAX_OPERATIONS
                || (amendment.target_updates.is_empty() && amendment.replacement_sources.is_none())
                || amendment
                    .target_updates
                    .iter()
                    .map(|value| value.operation_id)
                    .collect::<BTreeSet<_>>()
                    .len()
                    != amendment.target_updates.len()
                || amendment.target_updates.iter().any(|value| {
                    value.operation_id.is_nil()
                        || value.previous_expected_revision < 1
                        || value.replacement_guard.unit_id.is_nil()
                        || value.replacement_guard.revision < 1
                        || !text(&value.replacement_guard.rdf_digest, 256)
                        || !iri(&value.replacement_guard.unit_iri)
                        || !iri(&value.replacement_guard.revision_iri)
                })
            {
                return Err(Error::InvalidArguments);
            }
            if let Some(sources) = &amendment.replacement_sources {
                if sources.len() > DK2_MAX_LIST_ITEMS {
                    return Err(Error::CapacityExceeded);
                }
                for source in sources {
                    source.validate()?;
                }
            }
            Ok(())
        } else {
            Ok(())
        }
    }
}

impl CommitKnowledgeChange {
    pub fn validate(&self) -> Result<()> {
        request_identity(
            self.request_id,
            self.change_id,
            self.run_id,
            self.run_revision,
        )?;
        if self.seal_id.is_nil()
            || self.plan_revision < 1
            || !text(&self.plan_digest, 256)
            || !text(&self.sealed_command_digest, 256)
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl SettleKnowledgeChangeEffects {
    pub fn validate(&self) -> Result<()> {
        request_identity(
            self.request_id,
            self.change_id,
            self.run_id,
            self.run_revision,
        )?;
        if self.publisher_receipt_id.is_nil()
            || self.effect_ids.iter().any(uuid::Uuid::is_nil)
            || self.effect_ids.iter().collect::<BTreeSet<_>>().len() != self.effect_ids.len()
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}
