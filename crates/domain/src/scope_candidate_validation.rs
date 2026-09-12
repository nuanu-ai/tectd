use crate::{
    CandidateBoundary, CoverageResolutionKind, EmptyCandidateDispositionKind, Error, EvidenceKind,
    ProtectedChangeDisposition, Result, ScopeCandidateDraft,
};
use std::collections::BTreeSet;

impl ScopeCandidateDraft {
    pub fn validate(&self) -> Result<()> {
        if self.goals.len() > 100
            || self.candidates.len() > 100
            || self.evidence.len() > 100
            || self.blockers.len() > 100
            || self.protected_changes.len() > 100
        {
            return Err(Error::InvalidArguments);
        }
        let mut protected = BTreeSet::new();
        for change in &self.protected_changes {
            if change.accepted_evidence_id.is_nil()
                || change.prior_candidate_id.is_some_and(|id| id.is_nil())
                || change.authority_source_ref_id.is_nil()
                || !protected.insert((change.accepted_evidence_id, change.prior_candidate_id))
            {
                return Err(Error::InvalidArguments);
            }
            required(&change.rationale)?;
            match change.disposition {
                ProtectedChangeDisposition::Delete
                    if change.replacement_evidence.is_some()
                        || change.target_candidate.is_some() =>
                {
                    return Err(Error::InvalidArguments);
                }
                ProtectedChangeDisposition::Replace
                    if change.replacement_evidence.is_none()
                        || change.prior_candidate_id.is_some()
                            != change.target_candidate.is_some() =>
                {
                    return Err(Error::InvalidArguments);
                }
                ProtectedChangeDisposition::Reassociate
                    if change.prior_candidate_id.is_none()
                        || change.replacement_evidence.is_some()
                        || change.target_candidate.is_none() =>
                {
                    return Err(Error::InvalidArguments);
                }
                _ => {}
            }
            if let Some(reference) = &change.replacement_evidence {
                reference.validate()?;
            }
            if let Some(reference) = &change.target_candidate {
                reference.validate()?;
            }
        }
        let mut handles = BTreeSet::new();
        for identity in self
            .goals
            .iter()
            .map(|value| &value.identity)
            .chain(self.evidence.iter().map(|value| &value.identity))
            .chain(self.candidates.iter().map(|value| &value.identity))
            .chain(self.blockers.iter().map(|value| &value.identity))
        {
            identity.validate()?;
            if let Some(local) = &identity.local
                && !handles.insert(local)
            {
                return Err(Error::InvalidArguments);
            }
        }
        for goal in &self.goals {
            required(&goal.text)?;
            if goal.source_ref_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            goal.resolution.reference.validate()?;
        }
        for blocker in &self.blockers {
            required(&blocker.summary)?;
            if blocker.source_ref_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
        }
        for evidence in &self.evidence {
            required(&evidence.summary)?;
            if evidence.source_ref_id.is_nil()
                || evidence.kind == EvidenceKind::AcceptedWork
                    && evidence.authority_input_sequence.is_none()
            {
                return Err(Error::InvalidArguments);
            }
        }
        for candidate in &self.candidates {
            for text in [
                &candidate.title,
                &candidate.outcome,
                &candidate.trigger,
                &candidate.delivered_behavior,
                &candidate.proof,
            ] {
                required(text)?;
            }
            if candidate.coverage_goals.is_empty() {
                return Err(Error::InvalidArguments);
            }
            for reference in candidate
                .dependencies
                .iter()
                .chain(&candidate.coverage_goals)
                .chain(&candidate.evidence)
            {
                reference.validate()?;
            }
        }
        if self.boundary == CandidateBoundary::Finite && self.goals.is_empty() {
            return Err(Error::InvalidArguments);
        }
        match (&self.empty_disposition, self.candidates.is_empty()) {
            (None, true) | (Some(_), false) => return Err(Error::InvalidArguments),
            (Some(disposition), true) => {
                required(&disposition.reason)?;
                if disposition.source_ref_id.is_nil() {
                    return Err(Error::InvalidArguments);
                }
                match disposition.kind {
                    EmptyCandidateDispositionKind::AllCovered
                        if self.goals.is_empty()
                            || self.evidence.is_empty()
                            || !self.blockers.is_empty()
                            || self.pending_question.is_some()
                            || self.goals.iter().any(|goal| {
                                goal.resolution.kind != CoverageResolutionKind::Evidence
                            }) =>
                    {
                        return Err(Error::InvalidArguments);
                    }
                    EmptyCandidateDispositionKind::NeedsInput
                        if self
                            .pending_question
                            .as_ref()
                            .is_none_or(|question| question.trim().is_empty()) =>
                    {
                        return Err(Error::InvalidArguments);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn required(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
