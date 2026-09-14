use crate::*;
use std::collections::BTreeSet;

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.as_bytes().contains(&0)
}

fn list(values: &[String]) -> bool {
    values.len() <= DK_MAX_LIST_ITEMS && values.iter().all(|value| text(value, 4096))
}

impl KnowledgeConstraintDraft {
    pub fn validate(&self) -> Result<()> {
        if !text(&self.title, 1024)
            || !text(&self.statement, DK_MAX_TEXT_BYTES)
            || !text(&self.action, 4096)
            || !text(&self.target_iri, 4096)
            || !(self.target_iri.starts_with("http://")
                || self.target_iri.starts_with("https://")
                || self.target_iri.starts_with("urn:"))
            || !list(&self.conditions)
            || !list(&self.exceptions)
            || self.conditions.iter().collect::<BTreeSet<_>>().len() != self.conditions.len()
            || self.exceptions.iter().collect::<BTreeSet<_>>().len() != self.exceptions.len()
            || !text(&self.source.title, 1024)
            || !text(&self.source.uri, 4096)
            || !text(&self.source.text, DK_MAX_TEXT_BYTES)
        {
            return Err(Error::InvalidArguments);
        }
        if let KnowledgeBinding::SlicePhase {
            scope_id,
            slice_id,
            phase_id,
        } = &self.binding
            && (scope_id.is_nil() || slice_id.is_nil() || !text(phase_id, 256))
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

impl PrepareKnowledgeChange {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.expected_generation < 0
            || !text(&self.reason, 4096)
            || !text(&self.authority_basis, 4096)
        {
            return Err(Error::InvalidArguments);
        }
        match self.operation {
            KnowledgeOperation::Create
                if self.unit_id.is_none()
                    && self.expected_unit_revision.is_none()
                    && self.draft.is_some() => {}
            KnowledgeOperation::Revise
                if self.unit_id.is_some_and(|id| !id.is_nil())
                    && self.expected_unit_revision.is_some_and(|v| v >= 1)
                    && self.draft.is_some() => {}
            KnowledgeOperation::Retract
                if self.unit_id.is_some_and(|id| !id.is_nil())
                    && self.expected_unit_revision.is_some_and(|v| v >= 1)
                    && self.draft.is_none() => {}
            _ => return Err(Error::InvalidArguments),
        }
        if let Some(draft) = &self.draft {
            draft.validate()?;
        }
        Ok(())
    }
}

impl ReviewKnowledgeChange {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.change_id.is_nil()
            || self.change_revision < 1
            || !text(&self.proposal_digest, 256)
            || !text(&self.review_summary, DK_MAX_TEXT_BYTES)
            || !text(&self.method_read.id, 256)
            || !text(&self.method_read.version, 128)
            || !text(&self.method_read.digest, 256)
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl PublishKnowledgeChange {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.change_id.is_nil()
            || self.change_revision < 1
            || !text(&self.proposal_digest, 256)
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl KnowledgeContextQuery {
    pub fn validate(&self) -> Result<()> {
        match (self.unit_id, self.revision) {
            (None, None) => Ok(()),
            (Some(id), None) if !id.is_nil() => Ok(()),
            (Some(id), Some(revision)) if !id.is_nil() && revision >= 1 => Ok(()),
            _ => Err(Error::InvalidArguments),
        }
    }
}

impl RefreshPipelineKnowledge {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.run_id.is_nil()
            || self.run_revision < 1
            || !text(&self.phase_id, 256)
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}
