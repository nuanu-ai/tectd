use super::super::scope_source::canonical_digest;
use crate::{Result, ScopeAlternativeId, ScopeConstructorManifest, ScopeDigest};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatObligationLink {
    pub obligation_id: String,
    pub goal_id: Uuid,
}

/// One exact obligation in the phase-aware preservation universe. The identity
/// is stable across revisions; a changed body retains the identity and changes
/// its digest, so two different bodies cannot silently collapse into one item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AntiBloatProtectedObligation {
    pub id: String,
    pub content_digest: String,
    pub origin: AntiBloatObligationOrigin,
    /// Present when a persisted downstream effect belongs to this Scope
    /// candidate. It prevents removing a candidate with a live child graph.
    pub scope_candidate_id: Option<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiBloatObligationOrigin {
    ScopeSource,
    NativeScope,
    MatrixDeclaration,
    MatrixSelectedChoice,
    MatrixMappedNode,
    PipelineSelectedOption,
    MandatoryPolicy,
}

pub fn anti_bloat_protected_obligations_digest(
    digest: &impl ScopeDigest,
    obligations: &[AntiBloatProtectedObligation],
) -> Result<String> {
    canonical_digest(
        digest,
        "tect.anti-bloat-protected-obligations/1",
        &obligations,
    )
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
    /// Frozen source refs that the native resolver does not permit as goal
    /// citations. These remain mandatory source context, not goal links.
    pub non_goal_source_obligation_ids: Vec<String>,
    /// Mandatory policy is part of the frozen obligation universe, never an
    /// exemption from it.
    pub mandatory_policy_obligation_ids: Vec<String>,
    /// Canonical union of obligations that already exist at preparation time.
    /// Downstream Matrix and pipeline effects are absent until their own saved
    /// and verified transitions occur.
    pub protected_obligations: Vec<AntiBloatProtectedObligation>,
    pub protected_obligations_digest: String,
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
    pub protected_obligations_digest: String,
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
