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
            || self.supersessions.len() > 100
        {
            return Err(invalid("a draft list exceeds 100 items"));
        }
        let mut protected = BTreeSet::new();
        for (index, change) in self.protected_changes.iter().enumerate() {
            if change.accepted_evidence_id.is_nil()
                || change.prior_candidate_id.is_some_and(|id| id.is_nil())
                || change.authority_source_ref_id.is_nil()
                || !protected.insert((change.accepted_evidence_id, change.prior_candidate_id))
            {
                return Err(invalid(format!(
                    "protected_changes[{index}] has a nil id or repeats an accepted evidence change"
                )));
            }
            required(&change.rationale, "protected_changes.rationale")?;
            match change.disposition {
                ProtectedChangeDisposition::Delete
                    if change.replacement_evidence.is_some()
                        || change.target_candidate.is_some() =>
                {
                    return Err(invalid(format!(
                        "protected_changes[{index}]: delete takes no replacement_evidence or target_candidate"
                    )));
                }
                ProtectedChangeDisposition::Replace
                    if change.replacement_evidence.is_none()
                        || change.prior_candidate_id.is_some()
                            != change.target_candidate.is_some() =>
                {
                    return Err(invalid(format!(
                        "protected_changes[{index}]: replace needs replacement_evidence, and target_candidate exactly when prior_candidate_id is set"
                    )));
                }
                ProtectedChangeDisposition::Reassociate
                    if change.prior_candidate_id.is_none()
                        || change.replacement_evidence.is_some()
                        || change.target_candidate.is_none() =>
                {
                    return Err(invalid(format!(
                        "protected_changes[{index}]: reassociate needs prior_candidate_id and target_candidate and no replacement_evidence"
                    )));
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
                return Err(invalid(format!(
                    "local label `{local}` is used by more than one draft entity"
                )));
            }
        }
        for (index, goal) in self.goals.iter().enumerate() {
            required(&goal.text, "goals.text")?;
            if goal.source_ref_id.is_nil() {
                return Err(invalid(format!("goals[{index}].source_ref_id is nil")));
            }
            goal.resolution.reference.validate()?;
        }
        for (index, blocker) in self.blockers.iter().enumerate() {
            required(&blocker.summary, "blockers.summary")?;
            if blocker.source_ref_id.is_nil() {
                return Err(invalid(format!("blockers[{index}].source_ref_id is nil")));
            }
        }
        for (index, evidence) in self.evidence.iter().enumerate() {
            required(&evidence.summary, "evidence.summary")?;
            if evidence.source_ref_id.is_nil()
                || evidence.kind == EvidenceKind::AcceptedWork
                    && evidence.authority_input_sequence.is_none()
            {
                return Err(invalid(format!(
                    "evidence[{index}] needs a source_ref_id, and accepted_work needs authority_input_sequence"
                )));
            }
        }
        for (index, candidate) in self.candidates.iter().enumerate() {
            for (text, field) in [
                (&candidate.title, "candidates.title"),
                (&candidate.outcome, "candidates.outcome"),
                (&candidate.trigger, "candidates.trigger"),
                (
                    &candidate.delivered_behavior,
                    "candidates.delivered_behavior",
                ),
                (&candidate.proof, "candidates.proof"),
            ] {
                required(text, field)?;
            }
            require_candidate_coverage(!candidate.coverage_goals.is_empty())?;
            if candidate
                .change_rationale
                .as_ref()
                .is_some_and(|value| required(value, "candidates.change_rationale").is_err())
            {
                return Err(invalid(format!(
                    "candidates[{index}].change_rationale is blank; omit it or explain the change"
                )));
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
        let mut superseded = BTreeSet::new();
        for (index, value) in self.supersessions.iter().enumerate() {
            if value.candidate_id.is_nil()
                || value.revision < 1
                || !superseded.insert(value.candidate_id)
            {
                return Err(invalid(format!(
                    "supersessions[{index}] has a nil or repeated candidate_id or a revision below 1"
                )));
            }
            required(&value.reason, "supersessions.reason")?;
            for replacement in &value.replacements {
                replacement.validate()?;
            }
        }
        if self.boundary == CandidateBoundary::Finite && self.goals.is_empty() {
            return Err(invalid("a finite boundary needs at least one goal"));
        }
        match (&self.empty_disposition, self.candidates.is_empty()) {
            (None, true) | (Some(_), false) => {
                return Err(invalid(
                    "empty_disposition is required exactly when candidates is empty",
                ));
            }
            (Some(disposition), true) => {
                required(&disposition.reason, "empty_disposition.reason")?;
                if disposition.source_ref_id.is_nil() {
                    return Err(invalid("empty_disposition.source_ref_id is nil"));
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
                        return Err(invalid(
                            "all_covered needs goals and evidence, no blockers or pending question, and every goal resolved by evidence",
                        ));
                    }
                    EmptyCandidateDispositionKind::NeedsInput
                        if self
                            .pending_question
                            .as_ref()
                            .is_none_or(|question| question.trim().is_empty()) =>
                    {
                        return Err(invalid("needs_input needs a pending_question"));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn require_candidate_coverage(has_coverage: bool) -> Result<()> {
    if has_coverage {
        Ok(())
    } else {
        Err(Error::refused(
            crate::RefusalCode::CoverageIncomplete,
            "add_coverage",
            "coverage_goals",
        ))
    }
}

fn required(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        Err(invalid(format!("{field} is blank or contains NUL")))
    } else {
        Ok(())
    }
}

fn invalid(reason: impl std::fmt::Display) -> Error {
    Error::invalid_arguments_from(reason)
}

#[cfg(test)]
mod refusal_tests {
    use super::*;

    #[test]
    fn missing_candidate_coverage_has_typed_refusal() {
        let error = require_candidate_coverage(false).unwrap_err();
        assert_eq!(
            error.refusal().unwrap().code,
            crate::RefusalCode::CoverageIncomplete
        );
    }
}
