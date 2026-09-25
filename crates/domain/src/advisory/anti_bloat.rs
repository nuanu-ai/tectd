//! Pure, source-bound review of one saved candidate graph. Findings are advice;
//! only an exact caller-authored delta can reach the preservation checker.
use super::scope_source::{canonical_digest, valid_digest, valid_id};
use crate::{
    CandidateDeltaBatch, CandidateDeltaOperation, Error, ResolvedCandidateDraft, Result,
    ScopeAlternativeId, ScopeConstructorManifest, ScopeDigest,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatObligationLink {
    pub obligation_id: String,
    pub goal_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatInput {
    pub manifest: ScopeConstructorManifest,
    pub selected_id: ScopeAlternativeId,
    /// Revision of the actually saved selected draft, after the source-frozen
    /// manifest revision. This is the caller CAS revision, not source lineage.
    pub selected_revision: i64,
    /// Immutable identity of the trusted graph binding, supplied by its writer.
    pub graph_provenance: String,
    /// Digest of the authoritative dependency graph at review time.
    pub dependency_digest: String,
    /// Explicit source-to-goal witnesses supplied by the authoritative caller.
    pub obligation_links: Vec<AntiBloatObligationLink>,
    /// Mandatory policy is part of the frozen obligation universe, never an
    /// exemption from it.
    pub mandatory_policy_obligation_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiBloatClass {
    NecessaryResult,
    NecessaryEnabler,
    Duplicate,
    UnsupportedMechanism,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatFinding {
    pub id: String,
    pub candidate_id: Uuid,
    pub class: AntiBloatClass,
    pub rankable: bool,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatReview {
    pub source_digest: String,
    pub whole_set_digest: String,
    pub material_digest: String,
    pub candidate_set_id: Uuid,
    pub plan_revision: i64,
    pub dependency_digest: String,
    pub selected_id: ScopeAlternativeId,
    pub findings: Vec<AntiBloatFinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiBloatDisposition {
    Keep,
    Narrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiBloatRefusal {
    Stale,
    UnknownFinding,
    KeepForbidden,
    NotNarrowable,
    CoupledEditRequired,
    DestructiveMultiStep,
    ObligationLost,
    PlanMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatPreservation {
    pub source_digest: String,
    pub before_material_digest: String,
    pub after_material_digest: String,
    pub whole_set_digest: String,
    pub plan_revision: i64,
    pub dependency_digest: String,
    pub finding_id: String,
}

/// Receipt for a native saved-draft mutation, not a shadow candidate-delta
/// graph operation. The request ID addresses the ordinary candidate receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatApplyReceipt {
    pub review_id: Uuid,
    pub candidate_set_id: Uuid,
    pub idempotency_key: String,
    pub caller_request_id: Uuid,
    pub from_revision: i64,
    pub to_revision: i64,
    pub source_digest: String,
    pub before_material_digest: String,
    pub after_material_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiBloatVerificationVerdict {
    Pass,
    Fail,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiBloatVerificationReason {
    FullGraphPreserved,
    GraphOrReceiptMismatch,
    SourceEvidenceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatPreservationAttestation {
    pub request_id: Uuid,
    pub workspace_id: Uuid,
    pub review_id: Uuid,
    pub candidate_set_id: Uuid,
    pub from_revision: i64,
    pub to_revision: i64,
    pub verifier_principal_id: Uuid,
    pub verifier_session_id: Uuid,
    pub verdict: AntiBloatVerificationVerdict,
    pub reason: AntiBloatVerificationReason,
    pub evidence_digest: String,
    pub source_digest: String,
    pub before_material_digest: String,
    pub after_material_digest: String,
    pub caller_request_id: Uuid,
}

fn selected(input: &AntiBloatInput) -> Result<&crate::ScopeDecompositionAlternative> {
    input
        .manifest
        .eligible(&input.selected_id)
        .ok_or(Error::InvalidArguments)
}

fn validate_links(input: &AntiBloatInput) -> Result<BTreeMap<String, BTreeSet<Uuid>>> {
    let material = &selected(input)?.material;
    let obligations = input
        .manifest
        .obligations
        .iter()
        .map(|value| value.id.as_str())
        .collect::<BTreeSet<_>>();
    let policy = input
        .mandatory_policy_obligation_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if input.graph_provenance.trim().is_empty()
        || input.selected_revision <= input.manifest.source.candidate_set_revision
        || policy.len() != input.mandatory_policy_obligation_ids.len()
        || !policy.is_subset(&obligations)
        || !valid_digest(&input.dependency_digest)
    {
        return Err(Error::InvalidArguments);
    }
    let goals = material
        .goals
        .iter()
        .map(|goal| (goal.id, goal))
        .collect::<BTreeMap<_, _>>();
    let mut links: BTreeMap<String, BTreeSet<Uuid>> = BTreeMap::new();
    for link in &input.obligation_links {
        if !obligations.contains(link.obligation_id.as_str())
            || !valid_id(&link.obligation_id)
            || !goals.contains_key(&link.goal_id)
            || !links
                .entry(link.obligation_id.clone())
                .or_default()
                .insert(link.goal_id)
        {
            return Err(Error::InvalidArguments);
        }
    }
    if links.len() != obligations.len() {
        return Err(Error::InvalidSource);
    }
    Ok(links)
}

pub fn review_anti_bloat(
    digest: &impl ScopeDigest,
    input: &AntiBloatInput,
) -> Result<AntiBloatReview> {
    input.manifest.validate(digest)?;
    let links = validate_links(input)?;
    let alternative = selected(input)?;
    let material = &alternative.material;
    let required_goals = links.values().flatten().copied().collect::<BTreeSet<_>>();
    let mut findings = Vec::with_capacity(material.candidates.len());
    for candidate in &material.candidates {
        let has_required_goal = candidate
            .coverage_goal_ids
            .iter()
            .any(|id| required_goals.contains(id));
        let inbound = material
            .candidates
            .iter()
            .any(|other| other.id != candidate.id && other.dependencies.contains(&candidate.id));
        let protected = material.protected_changes.iter().any(|change| {
            change.prior_candidate_id == Some(candidate.id)
                || change.target_candidate_id == Some(candidate.id)
        });
        let duplicate = material.candidates.iter().any(|other| {
            other.id != candidate.id
                && other.outcome == candidate.outcome
                && other.trigger == candidate.trigger
                && other.delivered_behavior == candidate.delivered_behavior
                && other.proof == candidate.proof
        });
        let (class, reason) = if has_required_goal {
            (
                AntiBloatClass::NecessaryResult,
                "supports a frozen source obligation",
            )
        } else if inbound {
            (
                AntiBloatClass::NecessaryEnabler,
                "another candidate depends on it",
            )
        } else if protected || !candidate.evidence_ids.is_empty() {
            (
                AntiBloatClass::Unknown,
                "protected or evidence-bound work needs review",
            )
        } else if duplicate {
            (
                AntiBloatClass::Duplicate,
                "same result, trigger, behavior and proof as another candidate",
            )
        } else {
            (
                AntiBloatClass::UnsupportedMechanism,
                "no frozen obligation or dependency requires this candidate",
            )
        };
        let rankable = matches!(
            class,
            AntiBloatClass::Duplicate | AntiBloatClass::UnsupportedMechanism
        );
        let id = canonical_digest(
            digest,
            "tect.anti-bloat-finding/1",
            &(
                &input.manifest.source.digest,
                &input.manifest.whole_set_digest,
                &alternative.material_digest,
                &input.dependency_digest,
                candidate.id,
                class,
            ),
        )?;
        findings.push(AntiBloatFinding {
            id,
            candidate_id: candidate.id,
            class,
            rankable,
            reason: reason.into(),
        });
    }
    findings.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(AntiBloatReview {
        source_digest: input.manifest.source.digest.clone(),
        whole_set_digest: input.manifest.whole_set_digest.clone(),
        material_digest: alternative.material_digest.clone(),
        candidate_set_id: input.manifest.source.candidate_set_id,
        plan_revision: input.selected_revision,
        dependency_digest: input.dependency_digest.clone(),
        selected_id: input.selected_id.clone(),
        findings,
    })
}

/// Refuses every mutation unless a caller supplied one exact removal, its
/// resulting full graph, and an explicit disposition. No operation is applied.
pub fn check_anti_bloat_delta(
    digest: &impl ScopeDigest,
    input: &AntiBloatInput,
    review: &AntiBloatReview,
    finding_id: &str,
    disposition: AntiBloatDisposition,
    delta: &CandidateDeltaBatch,
    after: &ResolvedCandidateDraft,
) -> std::result::Result<AntiBloatPreservation, AntiBloatRefusal> {
    let (preservation, derived) =
        derive_anti_bloat_delta(digest, input, review, finding_id, disposition, delta)?;
    if &derived != after {
        return Err(AntiBloatRefusal::PlanMismatch);
    }
    Ok(preservation)
}

/// Derives the complete post-delta graph from the frozen authoritative graph.
/// The caller never supplies the graph used for preservation or persistence.
pub fn derive_anti_bloat_delta(
    digest: &impl ScopeDigest,
    input: &AntiBloatInput,
    review: &AntiBloatReview,
    finding_id: &str,
    disposition: AntiBloatDisposition,
    delta: &CandidateDeltaBatch,
) -> std::result::Result<(AntiBloatPreservation, ResolvedCandidateDraft), AntiBloatRefusal> {
    let current = review_anti_bloat(digest, input).map_err(|_| AntiBloatRefusal::Stale)?;
    if &current != review
        || delta.candidate_set_id != review.candidate_set_id
        || delta.expected_revision != review.plan_revision
    {
        return Err(AntiBloatRefusal::Stale);
    }
    let finding = review
        .findings
        .iter()
        .find(|item| item.id == finding_id)
        .ok_or(AntiBloatRefusal::UnknownFinding)?;
    if disposition == AntiBloatDisposition::Keep {
        return Err(AntiBloatRefusal::KeepForbidden);
    }
    if !finding.rankable {
        return Err(AntiBloatRefusal::NotNarrowable);
    }
    delta
        .validate()
        .map_err(|_| AntiBloatRefusal::PlanMismatch)?;
    if delta.operations.len() != 1 {
        return Err(AntiBloatRefusal::DestructiveMultiStep);
    }
    let CandidateDeltaOperation::CandidateRemove {
        candidate_id,
        expected_revision,
    } = &delta.operations[0]
    else {
        return Err(AntiBloatRefusal::DestructiveMultiStep);
    };
    let before = &selected(input)
        .map_err(|_| AntiBloatRefusal::Stale)?
        .material;
    let candidate = before
        .candidates
        .iter()
        .find(|item| item.id == *candidate_id)
        .ok_or(AntiBloatRefusal::PlanMismatch)?;
    if *candidate_id != finding.candidate_id || *expected_revision != candidate.revision {
        return Err(AntiBloatRefusal::PlanMismatch);
    }
    if before
        .candidates
        .iter()
        .any(|other| other.id != *candidate_id && other.dependencies.contains(candidate_id))
        || before.protected_changes.iter().any(|change| {
            change.prior_candidate_id == Some(*candidate_id)
                || change.target_candidate_id == Some(*candidate_id)
        })
    {
        return Err(AntiBloatRefusal::CoupledEditRequired);
    }
    let removed_goals = candidate
        .coverage_goal_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    if input
        .obligation_links
        .iter()
        .any(|link| removed_goals.contains(&link.goal_id))
    {
        return Err(AntiBloatRefusal::ObligationLost);
    }
    let mut expected = before.clone();
    expected.candidates.retain(|item| item.id != *candidate_id);
    expected
        .goals
        .retain(|goal| !removed_goals.contains(&goal.id));
    expected
        .delta
        .added
        .retain(|item| item.candidate_id != *candidate_id);
    expected
        .delta
        .changed
        .retain(|item| item.candidate_id != *candidate_id);
    expected
        .delta
        .unchanged
        .retain(|item| item.candidate_id != *candidate_id);
    if expected.validate().is_err() {
        return Err(AntiBloatRefusal::PlanMismatch);
    }
    let after_digest = super::scope_manifest::scope_candidate_material_digest(digest, &expected)
        .map_err(|_| AntiBloatRefusal::PlanMismatch)?;
    Ok((
        AntiBloatPreservation {
            source_digest: review.source_digest.clone(),
            before_material_digest: review.material_digest.clone(),
            after_material_digest: after_digest,
            whole_set_digest: review.whole_set_digest.clone(),
            plan_revision: review.plan_revision,
            dependency_digest: review.dependency_digest.clone(),
            finding_id: finding_id.into(),
        },
        expected,
    ))
}
