use crate::{
    KnowledgeDocumentBinding, KnowledgeDocumentDraft, KnowledgeEvidenceKind, KnowledgeKind,
    KnowledgeProfileId, KnowledgeSourceRef, PipelineDeliveryMode, PipelineRunStatus,
    PipelineSkillReadReceipt,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DK2_MAX_OPERATIONS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeLifecycleOperation {
    Create,
    Revise,
    Revalidate,
    Supersede,
    Retract,
    Erase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeLifecycleState {
    Active,
    Retracted,
    Superseded,
    ErasurePending,
    Erased,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KnowledgeChangePhaseId {
    KcIntake,
    KcResolveBaseline,
    KcQualifyPlan,
    KcQualifyEvidence,
    KcPrepareChange,
    KcDomainChecks,
    KcImpactPlan,
    KcReviewReconcile,
    KcPublicationGate,
    KcCommit,
    KcSettleEffects,
    KcResultHandoff,
}

impl KnowledgeChangePhaseId {
    pub const ALL: [Self; 12] = [
        Self::KcIntake,
        Self::KcResolveBaseline,
        Self::KcQualifyPlan,
        Self::KcQualifyEvidence,
        Self::KcPrepareChange,
        Self::KcDomainChecks,
        Self::KcImpactPlan,
        Self::KcReviewReconcile,
        Self::KcPublicationGate,
        Self::KcCommit,
        Self::KcSettleEffects,
        Self::KcResultHandoff,
    ];

    pub const fn ordinal(self) -> u32 {
        self as u32 + 1
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::KcIntake => "kc-intake",
            Self::KcResolveBaseline => "kc-resolve-baseline",
            Self::KcQualifyPlan => "kc-qualify-plan",
            Self::KcQualifyEvidence => "kc-qualify-evidence",
            Self::KcPrepareChange => "kc-prepare-change",
            Self::KcDomainChecks => "kc-domain-checks",
            Self::KcImpactPlan => "kc-impact-plan",
            Self::KcReviewReconcile => "kc-review-reconcile",
            Self::KcPublicationGate => "kc-publication-gate",
            Self::KcCommit => "kc-commit",
            Self::KcSettleEffects => "kc-settle-effects",
            Self::KcResultHandoff => "kc-result-handoff",
        }
    }

    pub const fn method_id(self) -> Option<&'static str> {
        match self {
            Self::KcIntake => Some("tect:knowledge-change:kc-intake"),
            Self::KcResolveBaseline => Some("tect:knowledge-change:kc-resolve-baseline"),
            Self::KcQualifyPlan => Some("tect:knowledge-change:kc-qualify-plan"),
            Self::KcQualifyEvidence => Some("tect:knowledge-change:kc-qualify-evidence"),
            Self::KcPrepareChange => Some("tect:knowledge-change:kc-prepare-change"),
            Self::KcDomainChecks => Some("tect:knowledge-change:kc-domain-checks"),
            Self::KcImpactPlan => Some("tect:knowledge-change:kc-impact-plan"),
            Self::KcReviewReconcile => Some("tect:knowledge-change:kc-review-reconcile"),
            Self::KcResultHandoff => Some("tect:knowledge-change:kc-result-handoff"),
            Self::KcPublicationGate | Self::KcCommit | Self::KcSettleEffects => None,
        }
    }

    pub const fn agent_authored(self) -> bool {
        matches!(
            self,
            Self::KcIntake
                | Self::KcResolveBaseline
                | Self::KcQualifyPlan
                | Self::KcQualifyEvidence
                | Self::KcPrepareChange
                | Self::KcDomainChecks
                | Self::KcImpactPlan
                | Self::KcReviewReconcile
                | Self::KcResultHandoff
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeChangeOwner {
    Workspace,
    PromotionSlice {
        scope_id: Uuid,
        slice_id: Uuid,
        slice_revision: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSearchRequirement {
    NotRequired,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeErasureRequirement {
    NotRequired,
    OwnedLiveCopies,
    RestoreSafe,
    AllRetainedCopies,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeCompletionRequirement {
    pub canonical_result: bool,
    pub exact_delivery: bool,
    pub impact_recorded: bool,
    pub search: KnowledgeSearchRequirement,
    pub erasure: KnowledgeErasureRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeOperationHint {
    pub client_label: String,
    pub operation: KnowledgeLifecycleOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_lifecycle: Option<KnowledgeLifecycleState>,
    pub reason: String,
    pub authority_basis: String,
    #[serde(default)]
    pub depends_on_labels: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginKnowledgeChange {
    pub request_id: Uuid,
    pub intent: String,
    pub desired_outcome: String,
    pub sources: Vec<crate::KnowledgeSourceRef>,
    pub operation_hints: Vec<KnowledgeOperationHint>,
    pub owner: KnowledgeChangeOwner,
    pub completion: KnowledgeCompletionRequirement,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery_mode: Option<PipelineDeliveryMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChangeRun {
    pub id: Uuid,
    pub change_id: Uuid,
    pub workspace_id: Uuid,
    pub revision: i64,
    pub definition_version: String,
    pub definition_digest: String,
    pub delivery_mode: PipelineDeliveryMode,
    pub status: PipelineRunStatus,
    pub current_phase_id: Option<KnowledgeChangePhaseId>,
    pub owner: KnowledgeChangeOwner,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRevisionGuard {
    pub unit_id: Uuid,
    pub revision: i64,
    pub lifecycle: KnowledgeLifecycleState,
    pub rdf_digest: String,
    pub unit_iri: String,
    pub revision_iri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeIdentityMatch {
    pub client_label: String,
    pub unit_id: Uuid,
    pub revision: i64,
    pub basis: String,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBaselineManifest {
    pub workspace_generation: i64,
    pub registry_generation: i64,
    pub policy_generation: i64,
    pub targets: Vec<KnowledgeRevisionGuard>,
    pub dependencies: Vec<KnowledgeRevisionGuard>,
    pub identity_matches: Vec<KnowledgeIdentityMatch>,
    pub source_availability: Vec<String>,
    #[serde(default)]
    pub assessment_conflicts: Vec<String>,
    #[serde(default)]
    pub assessment_gaps: Vec<String>,
    pub conflicts: Vec<String>,
    pub missing_context: Vec<String>,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChangeIntent {
    pub bounded_outcome: String,
    pub operation_hints: Vec<KnowledgeOperationHint>,
    pub authority_boundary: String,
    pub completion: KnowledgeCompletionRequirement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEvidenceClaim {
    pub operation_id: Uuid,
    pub claim: String,
    pub source_indexes: Vec<u32>,
    pub assumptions: Vec<String>,
    pub gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEvidenceManifest {
    pub claims: Vec<KnowledgeEvidenceClaim>,
    pub source_pins: Vec<KnowledgeResolvedSourcePin>,
    pub source_pin_digest: String,
    pub unresolved_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeResolvedSourcePin {
    pub source_index: u32,
    pub digest: String,
    pub evidence_kind: KnowledgeEvidenceKind,
    pub observed_at: Option<String>,
    pub evidence_scope: String,
    pub source_iri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeOperationQualification {
    pub operation_id: Uuid,
    pub knowledge_kind: KnowledgeKind,
    pub profiles: Vec<KnowledgeProfileId>,
    pub classification_basis: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePlanQualification {
    pub operations: Vec<KnowledgeOperationQualification>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeRevalidationDraft {
    pub sources: Vec<KnowledgeSourceRef>,
    pub evidence_basis: String,
    pub valid_until: Option<String>,
    pub review_due_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeSuccessorRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePlannedOperation {
    pub operation_id: Uuid,
    pub unit_id: Uuid,
    pub client_label: String,
    pub operation: KnowledgeLifecycleOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_lifecycle: Option<KnowledgeLifecycleState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<KnowledgeDocumentDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revalidation: Option<KnowledgeRevalidationDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub successor: Option<KnowledgeSuccessorRef>,
    pub replacement_bindings: Vec<KnowledgeDocumentBinding>,
    pub reason: String,
    pub authority_basis: String,
    pub dependency_operation_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binding_pins: Vec<KnowledgeResolvedBindingPin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeResolvedBindingPin {
    pub binding_index: u32,
    pub definition_kind: crate::PipelineKind,
    pub definition_version: String,
    pub definition_digest: String,
    pub phase_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeProposedChangeset {
    pub revision: i64,
    pub operations: Vec<KnowledgePlannedOperation>,
    pub semantic_diff: String,
    pub evidence_digest: String,
    pub digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeObligationDisposition {
    Satisfied,
    Reused,
    NotApplicable,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeObligationReceipt {
    pub operation_id: Uuid,
    pub profile_id: KnowledgeProfileId,
    pub obligation_id: String,
    pub disposition: KnowledgeObligationDisposition,
    pub reason: String,
    pub changeset_digest: String,
    pub method_reads: Vec<PipelineSkillReadReceipt>,
    pub input_digests: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_receipt_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeObligationReceipts {
    pub receipts: Vec<KnowledgeObligationReceipt>,
    pub unresolved_obligation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeImpactTarget {
    pub reference: String,
    pub owner_ref: String,
    pub effect: String,
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeImpactPlan {
    pub synchronous_changes: Vec<String>,
    pub affected_contexts: Vec<KnowledgeImpactTarget>,
    pub derivations: Vec<KnowledgeImpactTarget>,
    pub owned_copies: Vec<KnowledgeImpactTarget>,
    pub followups: Vec<KnowledgeImpactTarget>,
    pub blocking_conflicts: Vec<String>,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeReviewFinding {
    pub id: String,
    pub summary: String,
    pub owner_ref: String,
    pub revisit_phase_id: KnowledgeChangePhaseId,
    pub closed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closure_output_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeReviewOutcome {
    Ready,
    NoChange,
    Rejected,
    Findings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeReviewReceipt {
    pub outcome: KnowledgeReviewOutcome,
    pub reviewed_digests: Vec<String>,
    pub covered_operation_ids: Vec<Uuid>,
    pub covered_obligation_ids: Vec<String>,
    pub findings: Vec<KnowledgeReviewFinding>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeReadyToCommit {
    pub seal_id: Uuid,
    pub plan_revision: i64,
    pub plan_digest: String,
    pub changeset_digest: String,
    pub run_revision: i64,
    pub workspace_generation: i64,
    pub operation_ids: Vec<Uuid>,
    pub command_digest: String,
}
