use crate::{
    CandidateBoundary, CoverageResolutionKind, EmptyCandidateDispositionKind, Error, EvidenceKind,
    ProtectedChangeDisposition, ResolvedCandidateDraft, Result, ScopeCandidateDraft,
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

impl ResolvedCandidateDraft {
    /// Validate the saved, identity-resolved material before it can enter an
    /// advisory manifest. The preparation caller must separately check source
    /// ownership and freshness against the authoritative store.
    pub fn validate(&self) -> Result<()> {
        if self.goals.len() > 100
            || self.candidates.len() > 100
            || self.evidence.len() > 100
            || self.blockers.len() > 100
            || self.protected_changes.len() > 100
            || self.delta.superseded.len() > 100
        {
            return Err(invalid("a resolved draft list exceeds 100 items"));
        }
        let mut ids = BTreeSet::new();
        for (id, revision) in self
            .goals
            .iter()
            .map(|v| (v.id, v.revision))
            .chain(self.evidence.iter().map(|v| (v.id, v.revision)))
            .chain(self.candidates.iter().map(|v| (v.id, v.revision)))
            .chain(self.blockers.iter().map(|v| (v.id, v.revision)))
        {
            if id.is_nil() || revision < 1 || !ids.insert(id) {
                return Err(invalid(
                    "resolved entity has a nil or repeated id or invalid revision",
                ));
            }
        }
        let goal_ids = self.goals.iter().map(|v| v.id).collect::<BTreeSet<_>>();
        let evidence_ids = self.evidence.iter().map(|v| v.id).collect::<BTreeSet<_>>();
        let candidate_ids = self
            .candidates
            .iter()
            .map(|v| v.id)
            .collect::<BTreeSet<_>>();
        let blocker_ids = self.blockers.iter().map(|v| v.id).collect::<BTreeSet<_>>();
        for goal in &self.goals {
            required(&goal.text, "goals.text")?;
            if goal.source_ref_id.is_nil()
                || !match goal.resolution.kind {
                    CoverageResolutionKind::Candidate => {
                        candidate_ids.contains(&goal.resolution.id)
                    }
                    CoverageResolutionKind::Evidence => evidence_ids.contains(&goal.resolution.id),
                    CoverageResolutionKind::Blocker => blocker_ids.contains(&goal.resolution.id),
                }
            {
                return Err(invalid("goal source or resolution is invalid"));
            }
        }
        for evidence in &self.evidence {
            required(&evidence.summary, "evidence.summary")?;
            if evidence.source_ref_id.is_nil()
                || evidence.kind == EvidenceKind::AcceptedWork
                    && evidence.authority_input_sequence.is_none()
            {
                return Err(invalid("evidence source or authority input is invalid"));
            }
        }
        for blocker in &self.blockers {
            required(&blocker.summary, "blockers.summary")?;
            if blocker.source_ref_id.is_nil() {
                return Err(invalid("blocker source is invalid"));
            }
        }
        for candidate in &self.candidates {
            for (value, field) in [
                (&candidate.title, "candidates.title"),
                (&candidate.outcome, "candidates.outcome"),
                (&candidate.trigger, "candidates.trigger"),
                (
                    &candidate.delivered_behavior,
                    "candidates.delivered_behavior",
                ),
                (&candidate.proof, "candidates.proof"),
            ] {
                required(value, field)?;
            }
            require_candidate_coverage(!candidate.coverage_goal_ids.is_empty())?;
            if candidate.id == uuid::Uuid::nil()
                || candidate
                    .dependencies
                    .iter()
                    .any(|id| !candidate_ids.contains(id) || *id == candidate.id)
                || candidate
                    .coverage_goal_ids
                    .iter()
                    .any(|id| !goal_ids.contains(id))
                || candidate
                    .evidence_ids
                    .iter()
                    .any(|id| !evidence_ids.contains(id))
                || candidate
                    .coverage_goal_ids
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != candidate.coverage_goal_ids.len()
                || candidate.dependencies.iter().collect::<BTreeSet<_>>().len()
                    != candidate.dependencies.len()
                || candidate.evidence_ids.iter().collect::<BTreeSet<_>>().len()
                    != candidate.evidence_ids.len()
            {
                return Err(invalid("candidate references are invalid"));
            }
            if candidate.coverage_goal_ids.iter().any(|id| {
                self.goals.iter().any(|goal| {
                    goal.id == *id
                        && (goal.resolution.kind != CoverageResolutionKind::Candidate
                            || goal.resolution.id != candidate.id)
                })
            }) {
                return Err(invalid("candidate and goal coverage disagree"));
            }
        }
        if self.goals.iter().any(|goal| {
            goal.resolution.kind == CoverageResolutionKind::Candidate
                && !self.candidates.iter().any(|candidate| {
                    candidate.id == goal.resolution.id
                        && candidate.coverage_goal_ids.contains(&goal.id)
                })
        }) {
            return Err(invalid("goal and candidate coverage disagree"));
        }
        if self.boundary == CandidateBoundary::Finite && self.goals.is_empty() {
            return Err(invalid("a finite boundary needs at least one goal"));
        }
        let mut protected = BTreeSet::new();
        for change in &self.protected_changes {
            required(&change.rationale, "protected_changes.rationale")?;
            if change.accepted_evidence_id.is_nil()
                || change.authority_source_ref_id.is_nil()
                || change.prior_candidate_id.is_some_and(|id| id.is_nil())
                || !protected.insert((change.accepted_evidence_id, change.prior_candidate_id))
                || change
                    .replacement_evidence_id
                    .is_some_and(|id| !evidence_ids.contains(&id))
                || change
                    .target_candidate_id
                    .is_some_and(|id| !candidate_ids.contains(&id))
            {
                return Err(invalid("protected change identity or reference is invalid"));
            }
            match change.disposition {
                ProtectedChangeDisposition::Delete
                    if change.replacement_evidence_id.is_some()
                        || change.target_candidate_id.is_some() =>
                {
                    return Err(invalid("delete cannot name a replacement or target"));
                }
                ProtectedChangeDisposition::Replace
                    if change.replacement_evidence_id.is_none()
                        || change.prior_candidate_id.is_some()
                            != change.target_candidate_id.is_some() =>
                {
                    return Err(invalid("replace needs matching replacement references"));
                }
                ProtectedChangeDisposition::Reassociate
                    if change.prior_candidate_id.is_none()
                        || change.replacement_evidence_id.is_some()
                        || change.target_candidate_id.is_none() =>
                {
                    return Err(invalid("reassociate needs a prior and target candidate"));
                }
                _ => {}
            }
        }
        let mut classified = BTreeSet::new();
        for value in &self.delta.added {
            if value.candidate_id.is_nil()
                || value.revision < 1
                || !classified.insert(value.candidate_id)
                || !self.candidates.iter().any(|candidate| {
                    candidate.id == value.candidate_id && candidate.revision == value.revision
                })
            {
                return Err(invalid(
                    "delta.added is inconsistent with current candidates",
                ));
            }
        }
        for value in &self.delta.changed {
            required(&value.rationale, "delta.changed.rationale")?;
            if value.candidate_id.is_nil()
                || value.from_revision < 1
                || value.to_revision <= value.from_revision
                || !classified.insert(value.candidate_id)
                || !self.candidates.iter().any(|candidate| {
                    candidate.id == value.candidate_id && candidate.revision == value.to_revision
                })
            {
                return Err(invalid(
                    "delta.changed is inconsistent with current candidates",
                ));
            }
        }
        for value in &self.delta.unchanged {
            if value.candidate_id.is_nil()
                || value.revision < 1
                || !classified.insert(value.candidate_id)
                || !self.candidates.iter().any(|candidate| {
                    candidate.id == value.candidate_id && candidate.revision == value.revision
                })
            {
                return Err(invalid(
                    "delta.unchanged is inconsistent with current candidates",
                ));
            }
        }
        if classified != candidate_ids {
            return Err(invalid("delta omits a current candidate"));
        }
        let mut superseded = BTreeSet::new();
        for value in &self.delta.superseded {
            required(&value.reason, "delta.superseded.reason")?;
            for (text, field) in [
                (&value.prior.title, "delta.superseded.prior.title"),
                (&value.prior.outcome, "delta.superseded.prior.outcome"),
                (&value.prior.trigger, "delta.superseded.prior.trigger"),
                (
                    &value.prior.delivered_behavior,
                    "delta.superseded.prior.delivered_behavior",
                ),
                (&value.prior.proof, "delta.superseded.prior.proof"),
            ] {
                required(text, field)?;
            }
            if value.prior.id.is_nil()
                || value.prior.revision < 1
                || value.prior.coverage_goal_ids.is_empty()
                || value.prior.dependencies.iter().any(uuid::Uuid::is_nil)
                || value.prior.coverage_goal_ids.iter().any(uuid::Uuid::is_nil)
                || value.prior.evidence_ids.iter().any(uuid::Uuid::is_nil)
                || value
                    .prior
                    .dependencies
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != value.prior.dependencies.len()
                || value
                    .prior
                    .coverage_goal_ids
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != value.prior.coverage_goal_ids.len()
                || value
                    .prior
                    .evidence_ids
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != value.prior.evidence_ids.len()
                || candidate_ids.contains(&value.prior.id)
                || !superseded.insert(value.prior.id)
                || value
                    .replacement_candidate_ids
                    .iter()
                    .any(|id| !candidate_ids.contains(id))
                || value
                    .replacement_candidate_ids
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
                    != value.replacement_candidate_ids.len()
            {
                return Err(invalid(
                    "delta.superseded has invalid prior or replacements",
                ));
            }
        }
        match (&self.empty_disposition, self.candidates.is_empty()) {
            (None, true) | (Some(_), false) => {
                return Err(invalid(
                    "empty_disposition is required exactly when candidates is empty",
                ));
            }
            (Some(value), true) => {
                required(&value.reason, "empty_disposition.reason")?;
                if value.source_ref_id.is_nil()
                    || value.kind == EmptyCandidateDispositionKind::NeedsInput
                        && self
                            .pending_question
                            .as_ref()
                            .is_none_or(|q| q.trim().is_empty())
                {
                    return Err(invalid("empty disposition is incomplete"));
                }
                if value.kind == EmptyCandidateDispositionKind::AllCovered
                    && (self.goals.is_empty()
                        || self.evidence.is_empty()
                        || !self.blockers.is_empty()
                        || self.pending_question.is_some()
                        || self
                            .goals
                            .iter()
                            .any(|goal| goal.resolution.kind != CoverageResolutionKind::Evidence))
                {
                    return Err(invalid("all_covered disposition is inconsistent"));
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
