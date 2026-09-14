use crate::{
    KnowledgeBaselineManifest, KnowledgeChangeIntent, KnowledgeChangePhaseId, KnowledgeChangeRun,
    KnowledgeDocumentDraft, KnowledgeEvidenceManifest, KnowledgeImpactPlan,
    KnowledgeLifecycleOperation, KnowledgeLifecycleState, KnowledgeObligationReceipts,
    KnowledgeProposedChangeset, KnowledgeRdfDigestScope, KnowledgeReadyToCommit,
    KnowledgeReviewFinding, KnowledgeReviewReceipt, KnowledgeRevisionGuard, PipelineConsumedInput,
    PipelineConsumedOutput, PipelinePhaseOutcome, PipelineRunStatus, PipelineSkillReadReceipt,
    PipelineTransition,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeEffectKind {
    ExactDelivery,
    Invalidation,
    Impact,
    Search,
    VisibilityClosure,
    OwnedCopyPurge,
    BackupDisposition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeEffectStatus {
    NotApplicable,
    NotConfigured,
    Pending,
    Ready,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEffectReceipt {
    pub effect_id: Uuid,
    pub kind: KnowledgeEffectKind,
    pub status: KnowledgeEffectStatus,
    pub generation: i64,
    pub owner_ref: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeAppliedOperationReceipt {
    pub operation_id: Uuid,
    pub unit_id: Uuid,
    pub operation: KnowledgeLifecycleOperation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    pub event_id: Uuid,
    pub unit_iri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision_iri: Option<String>,
    pub event_iri: String,
    pub rdf_digest: String,
    pub rdf_digest_method: String,
    pub rdf_digest_scope: KnowledgeRdfDigestScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePublisherReceipt {
    pub id: Uuid,
    pub request_id: Uuid,
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub sealed_command_digest: String,
    pub workspace_generation: i64,
    pub applied_operations: Vec<KnowledgeAppliedOperationReceipt>,
    pub effects: Vec<KnowledgeEffectReceipt>,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeErasedOperationReceipt {
    pub operation_id: Uuid,
    pub unit_id: Uuid,
    pub operation: KnowledgeLifecycleOperation,
    pub event_id: Uuid,
    pub erasure_sequence: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "receipt", rename_all = "snake_case")]
pub enum KnowledgeRetainedOperationReceipt {
    Intact(KnowledgeAppliedOperationReceipt),
    PayloadErased(KnowledgeErasedOperationReceipt),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeOpaqueEffectReceipt {
    pub effect_id: Uuid,
    pub kind: KnowledgeEffectKind,
    pub status: KnowledgeEffectStatus,
    pub generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeErasedEffectsReport {
    pub publisher_receipt_id: Uuid,
    pub effects: Vec<KnowledgeOpaqueEffectReceipt>,
    pub required_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeErasedResult {
    pub canonical: KnowledgeCanonicalOutcome,
    pub user_outcome: KnowledgeUserOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_receipt_id: Option<Uuid>,
    pub effects: Vec<KnowledgeOpaqueEffectReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeErasedPublisherReceipt {
    pub id: Uuid,
    pub request_id: Uuid,
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub completion: crate::KnowledgeCompletionRequirement,
    pub operations: Vec<KnowledgeRetainedOperationReceipt>,
    pub effects: Vec<KnowledgeOpaqueEffectReceipt>,
}

fn public_effect(value: &KnowledgeOpaqueEffectReceipt) -> KnowledgeEffectReceipt {
    KnowledgeEffectReceipt {
        effect_id: value.effect_id,
        kind: value.kind,
        status: value.status,
        generation: value.generation,
        owner_ref: "tect-backend".into(),
        detail: "Semantic payload erased; this is the preserved backend effect status.".into(),
    }
}

fn opaque_effect(value: &KnowledgeEffectReceipt) -> KnowledgeOpaqueEffectReceipt {
    KnowledgeOpaqueEffectReceipt {
        effect_id: value.effect_id,
        kind: value.kind,
        status: value.status,
        generation: value.generation,
    }
}

fn remaining_effects(values: &[KnowledgeOpaqueEffectReceipt]) -> Vec<String> {
    values
        .iter()
        .filter(|value| {
            matches!(
                value.status,
                KnowledgeEffectStatus::Pending | KnowledgeEffectStatus::Failed
            )
        })
        .map(|value| format!("{:?}", value.kind).to_lowercase())
        .collect()
}

impl KnowledgeErasedEffectsReport {
    pub fn from_verified(value: &KnowledgeEffectsReport) -> Self {
        Self {
            publisher_receipt_id: value.publisher_receipt_id,
            effects: value.effects.iter().map(opaque_effect).collect(),
            required_complete: value.required_complete,
        }
    }

    pub fn to_public(&self) -> KnowledgeEffectsReport {
        KnowledgeEffectsReport {
            publisher_receipt_id: self.publisher_receipt_id,
            effects: self.effects.iter().map(public_effect).collect(),
            required_complete: self.required_complete,
            remaining_work: remaining_effects(&self.effects),
        }
    }
}

impl KnowledgeErasedResult {
    pub fn from_validated(value: &KnowledgeChangeResult) -> Self {
        Self {
            canonical: value.canonical,
            user_outcome: value.user_outcome,
            publisher_receipt_id: value.publisher_receipt_id,
            effects: value.effects.iter().map(opaque_effect).collect(),
        }
    }

    pub fn to_public(&self) -> KnowledgeChangeResult {
        KnowledgeChangeResult {
            canonical: self.canonical,
            user_outcome: self.user_outcome,
            summary: "Knowledge change completed after its owned semantic payload was erased."
                .into(),
            remaining_work: remaining_effects(&self.effects),
            publisher_receipt_id: self.publisher_receipt_id,
            effects: self.effects.iter().map(public_effect).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePublicationReference {
    pub change_id: Uuid,
    pub publisher_receipt_id: Uuid,
    pub publisher_receipt_digest: String,
    pub operation_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeEffectsReport {
    pub publisher_receipt_id: Uuid,
    pub effects: Vec<KnowledgeEffectReceipt>,
    pub required_complete: bool,
    pub remaining_work: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeCanonicalOutcome {
    NotApplied,
    Applied,
    NoChange,
    Rejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeUserOutcome {
    Achieved,
    NotAchieved,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChangeResult {
    pub canonical: KnowledgeCanonicalOutcome,
    pub user_outcome: KnowledgeUserOutcome,
    pub summary: String,
    pub remaining_work: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher_receipt_id: Option<Uuid>,
    pub effects: Vec<KnowledgeEffectReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", content = "data", rename_all = "kebab-case")]
pub enum KnowledgeAgentPhaseData {
    KcIntake(KnowledgeChangeIntent),
    KcResolveBaseline(KnowledgeBaselineManifest),
    KcQualifyPlan(crate::KnowledgePlanQualification),
    KcQualifyEvidence(KnowledgeEvidenceManifest),
    KcPrepareChange(KnowledgeProposedChangeset),
    KcDomainChecks(KnowledgeObligationReceipts),
    KcImpactPlan(KnowledgeImpactPlan),
    KcReviewReconcile(KnowledgeReviewReceipt),
    KcResultHandoff(KnowledgeChangeResult),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeAgentPhaseOutputDraft {
    pub phase_id: KnowledgeChangePhaseId,
    pub expected_run_revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phasewise_reason: Option<String>,
    pub plan_revision: i64,
    pub plan_digest: String,
    pub consumed_outputs: Vec<PipelineConsumedOutput>,
    pub consumed_inputs: Vec<PipelineConsumedInput>,
    pub baseline_guards: Vec<KnowledgeRevisionGuard>,
    pub source_digests: Vec<String>,
    pub method_reads: Vec<PipelineSkillReadReceipt>,
    pub body: String,
    pub data: KnowledgeAgentPhaseData,
    pub verdict: String,
    pub outcome: PipelinePhaseOutcome,
    pub transition: PipelineTransition,
    pub findings: Vec<KnowledgeReviewFinding>,
    pub dispositions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePhaseOutput {
    pub id: Uuid,
    pub run_id: Uuid,
    pub revision: i64,
    pub digest: String,
    pub output: KnowledgeAgentPhaseOutputDraft,
    pub stale: bool,
    pub stale_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePhaseAttempt {
    pub id: Uuid,
    pub run_id: Uuid,
    pub phase_id: KnowledgeChangePhaseId,
    pub attempt: i64,
    pub outcome: PipelinePhaseOutcome,
    pub transition: PipelineTransition,
    pub output_id: Option<Uuid>,
    pub output_digest: Option<String>,
    pub actor_session_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChangeInput {
    pub id: Uuid,
    pub sequence: i64,
    pub revisit_phase_id: KnowledgeChangePhaseId,
    pub reason: String,
    pub input: String,
    pub digest: String,
    pub actor_session_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_basis_amendment: Option<KnowledgeAppliedBasisAmendment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeTargetBasisUpdate {
    pub operation_id: Uuid,
    pub previous_expected_revision: i64,
    pub previous_expected_lifecycle: KnowledgeLifecycleState,
    pub replacement_guard: KnowledgeRevisionGuard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeBasisAmendment {
    pub target_updates: Vec<KnowledgeTargetBasisUpdate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement_sources: Option<Vec<crate::KnowledgeSourceRef>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSourceBasisChange {
    pub previous_sources: Vec<crate::KnowledgeSourceRef>,
    pub previous_pins: Vec<crate::KnowledgeResolvedSourcePin>,
    pub previous_source_revision: i64,
    pub replacement_sources: Vec<crate::KnowledgeSourceRef>,
    pub replacement_pins: Vec<crate::KnowledgeResolvedSourcePin>,
    pub replacement_source_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeAppliedBasisAmendment {
    pub target_updates: Vec<KnowledgeTargetBasisUpdate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_change: Option<KnowledgeSourceBasisChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgePayloadKind {
    RequestReplay,
    PhaseOutput,
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePayloadTombstone {
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub payload_id: Uuid,
    pub kind: KnowledgePayloadKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeOperationAssignment {
    pub operation_id: Uuid,
    pub client_label: String,
    pub operation: KnowledgeLifecycleOperation,
    pub unit_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_lifecycle: Option<KnowledgeLifecycleState>,
    pub dependency_operation_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChangeOrigin {
    pub intent: String,
    pub desired_outcome: String,
    pub owner: crate::KnowledgeChangeOwner,
    pub completion: crate::KnowledgeCompletionRequirement,
    pub sources: Vec<crate::KnowledgeSourceRef>,
    pub source_pins: Vec<crate::KnowledgeResolvedSourcePin>,
    #[serde(default)]
    pub source_revision: i64,
    pub operation_hints: Vec<crate::KnowledgeOperationHint>,
    pub operations: Vec<KnowledgeOperationAssignment>,
}

mod api;
pub use api::*;
