use super::scope_source::canonical_digest;
use crate::{
    Error, FrozenScopeSource, GuardedScopeAdvice, Result, ScopeAdviceId, ScopeAlternativeId,
    ScopeConstructorManifest, ScopeDigest,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeDispositionAction {
    Accept,
    RejectAll,
    SupersedeWithDeterministicChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeDispositionItemState {
    Selected,
    NotSelected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeDispositionItem {
    pub alternative_id: ScopeAlternativeId,
    pub state: ScopeDispositionItemState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeDispositionRequest {
    pub request_id: Uuid,
    pub advice_id: ScopeAdviceId,
    pub expected_revision: i64,
    pub action: ScopeDispositionAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_id: Option<ScopeAlternativeId>,
    pub items: Vec<ScopeDispositionItem>,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDispositionRevision {
    pub id: Uuid,
    pub request_id: Uuid,
    pub advice_id: ScopeAdviceId,
    pub revision: i64,
    pub supersedes_id: Option<Uuid>,
    pub action: ScopeDispositionAction,
    pub selected_id: Option<ScopeAlternativeId>,
    pub items: Vec<ScopeDispositionItem>,
    pub rationale: String,
}

impl ScopeDispositionRequest {
    pub fn validate(
        &self,
        digest: &impl ScopeDigest,
        manifest: &ScopeConstructorManifest,
        advice: &GuardedScopeAdvice,
        current: Option<&ScopeDispositionRevision>,
    ) -> Result<()> {
        manifest.validate(digest)?;
        crate::validate_guarded_advice_binding(digest, manifest, advice)?;
        self.advice_id.validate()?;
        if self.advice_id != advice.id {
            return Err(Error::InputConflict);
        }
        if self.expected_revision < 0
            || current.map_or(0, |value| value.revision) != self.expected_revision
            || current.is_some_and(|value| value.advice_id != advice.id)
        {
            return Err(Error::StaleRevision);
        }
        if current.is_some_and(|value| value.request_id == self.request_id) {
            return Err(Error::InputConflict);
        }
        if self.request_id.is_nil()
            || self.rationale.trim().is_empty()
            || self.rationale.len() > 4096
        {
            return Err(Error::InvalidArguments);
        }
        validate_disposition_items(
            manifest,
            self.action,
            self.selected_id.as_ref(),
            &self.items,
        )
    }

    pub fn into_revision(
        self,
        id: Uuid,
        digest: &impl ScopeDigest,
        manifest: &ScopeConstructorManifest,
        advice: &GuardedScopeAdvice,
        current: Option<&ScopeDispositionRevision>,
    ) -> Result<ScopeDispositionRevision> {
        self.validate(digest, manifest, advice, current)?;
        if id.is_nil() || current.is_some_and(|value| value.id == id) {
            return Err(Error::InvalidArguments);
        }
        Ok(ScopeDispositionRevision {
            id,
            request_id: self.request_id,
            advice_id: self.advice_id,
            revision: self
                .expected_revision
                .checked_add(1)
                .ok_or(Error::InvalidArguments)?,
            supersedes_id: current.map(|value| value.id),
            action: self.action,
            selected_id: self.selected_id,
            items: self.items,
            rationale: self.rationale,
        })
    }
}

impl ScopeDispositionRevision {
    pub fn validate(
        &self,
        digest: &impl ScopeDigest,
        manifest: &ScopeConstructorManifest,
        advice: &GuardedScopeAdvice,
    ) -> Result<()> {
        manifest.validate(digest)?;
        crate::validate_guarded_advice_binding(digest, manifest, advice)?;
        if self.id.is_nil()
            || self.request_id.is_nil()
            || self.advice_id != advice.id
            || self.revision < 1
            || (self.revision == 1) != self.supersedes_id.is_none()
            || self
                .supersedes_id
                .is_some_and(|id| id.is_nil() || id == self.id)
            || self.rationale.trim().is_empty()
            || self.rationale.len() > 4096
        {
            return Err(Error::InvalidArguments);
        }
        validate_disposition_items(
            manifest,
            self.action,
            self.selected_id.as_ref(),
            &self.items,
        )
    }
}

fn validate_disposition_items(
    manifest: &ScopeConstructorManifest,
    action: ScopeDispositionAction,
    selected_id: Option<&ScopeAlternativeId>,
    items: &[ScopeDispositionItem],
) -> Result<()> {
    let expected = manifest
        .emitted
        .iter()
        .map(|value| &value.id)
        .collect::<BTreeSet<_>>();
    let mut actual = BTreeMap::new();
    for item in items {
        if actual.insert(&item.alternative_id, item.state).is_some() {
            return Err(Error::InvalidArguments);
        }
    }
    if actual.keys().copied().collect::<BTreeSet<_>>() != expected {
        return Err(Error::InvalidArguments);
    }
    let selected = actual
        .iter()
        .filter_map(|(id, state)| {
            matches!(state, ScopeDispositionItemState::Selected).then_some(*id)
        })
        .collect::<Vec<_>>();
    match action {
        ScopeDispositionAction::RejectAll if selected_id.is_none() && selected.is_empty() => Ok(()),
        ScopeDispositionAction::Accept
            if selected_id.is_some() && selected == selected_id.into_iter().collect::<Vec<_>>() =>
        {
            Ok(())
        }
        ScopeDispositionAction::SupersedeWithDeterministicChoice
            if selected_id == Some(&manifest.baseline_id)
                && selected == vec![&manifest.baseline_id] =>
        {
            Ok(())
        }
        _ => Err(Error::InvalidArguments),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshScopeObservation {
    pub source: FrozenScopeSource,
    pub manifest: ScopeConstructorManifest,
    pub candidate_set_revision: i64,
    pub advice_id: ScopeAdviceId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopePreservationStatus {
    Passed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopePreservationResult {
    pub status: ScopePreservationStatus,
    pub selected_id: ScopeAlternativeId,
    pub coverage_digest: String,
    pub reason_codes: Vec<String>,
}

pub fn evaluate_scope_preservation(
    digest: &impl ScopeDigest,
    prepared_manifest: &ScopeConstructorManifest,
    advice: &GuardedScopeAdvice,
    disposition: &ScopeDispositionRevision,
    fresh: &FreshScopeObservation,
) -> Result<ScopePreservationResult> {
    prepared_manifest.validate(digest)?;
    crate::validate_guarded_advice_binding(digest, prepared_manifest, advice)?;
    disposition.validate(digest, prepared_manifest, advice)?;
    fresh.source.validate(digest)?;
    fresh.manifest.validate(digest)?;
    fresh.advice_id.validate()?;
    if fresh.manifest.source != fresh.source
        || fresh.candidate_set_revision != fresh.source.candidate_set_revision
    {
        return Err(Error::InvalidArguments);
    }
    let selected_id = disposition
        .selected_id
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let alternative = prepared_manifest
        .eligible(selected_id)
        .ok_or(Error::InvalidArguments)?;
    let mut coverage = alternative.coverage.clone();
    for row in &mut coverage {
        row.condition_ids.sort();
        row.exception_ids.sort();
    }
    coverage.sort_by(|left, right| left.obligation_id.cmp(&right.obligation_id));
    let coverage_digest = canonical_digest(digest, "tect.scope-disposition-coverage/1", &coverage)?;
    let mut reasons = Vec::new();
    if fresh.source.digest != prepared_manifest.source.digest {
        reasons.push("source_changed".into());
    }
    if fresh.manifest.whole_set_digest != prepared_manifest.whole_set_digest {
        reasons.push("manifest_changed".into());
    }
    if fresh.manifest.eligible_set_digest != prepared_manifest.eligible_set_digest {
        reasons.push("eligible_set_changed".into());
    }
    if fresh.candidate_set_revision != prepared_manifest.source.candidate_set_revision {
        reasons.push("candidate_revision_changed".into());
    }
    if fresh.advice_id != advice.id {
        reasons.push("advice_changed".into());
    }
    Ok(ScopePreservationResult {
        status: if reasons.is_empty() {
            ScopePreservationStatus::Passed
        } else {
            ScopePreservationStatus::Failed
        },
        selected_id: selected_id.clone(),
        coverage_digest,
        reason_codes: reasons,
    })
}
