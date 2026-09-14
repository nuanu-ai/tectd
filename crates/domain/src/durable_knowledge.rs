use crate::{PipelineDefinitionSnapshot, PipelineKind};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DK_PROFILE_ID: &str = "tect:durable-knowledge:general-constraint";
pub const DK_PROFILE_VERSION: &str = "dk-1";
pub const DK_PREPARATION_METHOD_ID: &str = "tect:knowledge-change-prepare";
pub const DK_REVIEW_METHOD_ID: &str = "tect:knowledge-change-review";
pub const DK_METHOD_VERSION: &str = "dk-1";
pub const DK_MAX_TEXT_BYTES: usize = 65_536;
pub const DK_MAX_DRAFT_BYTES: usize = 131_072;
pub const DK_MAX_LIST_ITEMS: usize = 64;
pub const DK_MAX_MANIFEST_BYTES: usize = 1_048_576;

pub const DK_PREPARATION_METHOD_BODY: &str = "Prepare exactly one source-derived General-DK constraint operation. Preserve the source text, normative modality, every material condition and exception, the semantic target/action, execution binding, reason and authority basis. For revise or retract, pin the current accepted unit revision and workspace generation; never auto-rebase. A prepared proposal is review-required and has no canonical knowledge effect.";
pub const DK_REVIEW_METHOD_BODY: &str = "Read the exact proposal, its source snapshot and baseline. Preserve every material condition, exception and normative modality; distinguish source assertions from your interpretation. Check the stated execution binding and target/action, changes to prior meaning, current authority and contradictions in the selected knowledge. Report unresolved findings explicitly. Approve only the exact proposal digest whose source trace and scope you have reviewed; a filled schema or a successful RDF parse is not semantic evidence. For retract, review the withdrawal authority, target and impact without requiring a replacement or re-proving the withdrawn claim. Never execute a described operation or construct an unsolicited audit/test harness merely to publish its description.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeOperation {
    Create,
    Revise,
    Retract,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeModality {
    Must,
    MustNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeVersionResolution {
    CurrentAccepted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgePurpose {
    ExecutionConstraint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeBindingKind {
    Workspace,
    SlicePhase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeBinding {
    Workspace,
    SlicePhase {
        scope_id: Uuid,
        slice_id: Uuid,
        phase_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSourceSnapshot {
    pub title: String,
    pub uri: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeConstraintDraft {
    pub title: String,
    pub statement: String,
    pub modality: KnowledgeModality,
    pub action: String,
    pub target_iri: String,
    pub conditions: Vec<String>,
    pub exceptions: Vec<String>,
    pub source: KnowledgeSourceSnapshot,
    pub binding: KnowledgeBinding,
    pub purpose: KnowledgePurpose,
    pub version_resolution: KnowledgeVersionResolution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeBindingProvenance {
    pub definition_kind: PipelineKind,
    pub definition_version: String,
    pub definition_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMethodSnapshot {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMethodReadReceipt {
    pub id: String,
    pub version: String,
    pub digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeChangeStage {
    ReviewRequired,
    ReadyToPublish,
    Rejected,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeReviewVerdict {
    Approve,
    Reject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeRdfDigestScope {
    RevisionPublicationPayload,
    LifecycleEventPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeReview {
    pub verdict: KnowledgeReviewVerdict,
    pub summary: String,
    pub method_read: KnowledgeMethodReadReceipt,
    pub reviewer_principal_id: Uuid,
    pub reviewer_session_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChange {
    pub id: Uuid,
    pub unit_id: Uuid,
    pub change_revision: i64,
    pub operation: KnowledgeOperation,
    pub stage: KnowledgeChangeStage,
    pub expected_generation: i64,
    pub expected_unit_revision: Option<i64>,
    pub proposed_unit_revision: i64,
    pub proposal_digest: String,
    pub source_sha256: Option<String>,
    pub semantic_diff: String,
    pub baseline: Option<KnowledgeUnitRevision>,
    pub proposal: Option<KnowledgeConstraintDraft>,
    pub binding_provenance: Option<KnowledgeBindingProvenance>,
    pub preparation_method: KnowledgeMethodSnapshot,
    pub review_method: KnowledgeMethodSnapshot,
    pub reason: String,
    pub authority_basis: String,
    pub review: Option<KnowledgeReview>,
    pub publication_receipt: Option<KnowledgePublicationReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareKnowledgeChange {
    pub request_id: Uuid,
    pub operation: KnowledgeOperation,
    #[serde(default)]
    pub unit_id: Option<Uuid>,
    #[serde(default)]
    pub expected_unit_revision: Option<i64>,
    pub expected_generation: i64,
    #[serde(default)]
    pub draft: Option<KnowledgeConstraintDraft>,
    pub reason: String,
    pub authority_basis: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrepareKnowledgeChangeOutcome {
    Prepared(KnowledgeChange),
    Replay(KnowledgeChange),
    Duplicate { existing_unit_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewKnowledgeChange {
    pub request_id: Uuid,
    pub change_id: Uuid,
    pub change_revision: i64,
    pub proposal_digest: String,
    pub verdict: KnowledgeReviewVerdict,
    pub review_summary: String,
    pub method_read: KnowledgeMethodReadReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKnowledgeChangeOutcome {
    Approved(KnowledgeChange),
    Rejected(KnowledgeChange),
    Replay(KnowledgeChange),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishKnowledgeChange {
    pub request_id: Uuid,
    pub change_id: Uuid,
    pub change_revision: i64,
    pub proposal_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgePublicationReceipt {
    pub id: Uuid,
    pub change_id: Uuid,
    pub unit_id: Uuid,
    pub operation: KnowledgeOperation,
    pub unit_revision: i64,
    pub event_id: Uuid,
    pub workspace_generation: i64,
    pub rdf_digest: String,
    pub rdf_digest_method: String,
    pub rdf_digest_scope: KnowledgeRdfDigestScope,
    pub unit_iri: String,
    pub revision_iri: String,
    pub event_iri: String,
    pub delivery_eligible: bool,
    pub effects_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishKnowledgeChangeOutcome {
    Published(KnowledgePublicationReceipt),
    Replay(KnowledgePublicationReceipt),
    Duplicate { existing_unit_id: Uuid },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeContextQuery {
    #[serde(default)]
    pub unit_id: Option<Uuid>,
    #[serde(default)]
    pub revision: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeCapability {
    pub ready: bool,
    pub profile_id: String,
    pub profile_version: String,
    pub pgrdf_version: Option<String>,
    pub supported_operations: Vec<KnowledgeOperation>,
    pub supported_bindings: Vec<KnowledgeBindingKind>,
    pub lifecycle_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeUnitRevision {
    pub unit_id: Uuid,
    pub revision: i64,
    pub active: bool,
    pub constraint: KnowledgeConstraintDraft,
    pub source_sha256: String,
    pub rdf_digest: String,
    pub rdf_digest_method: String,
    pub rdf_digest_scope: KnowledgeRdfDigestScope,
    pub publication_event_id: Uuid,
    pub unit_iri: String,
    pub revision_iri: String,
    pub source_iri: String,
    pub publication_event_iri: String,
    pub publication_operation: KnowledgeOperation,
    pub publication_reason: String,
    pub publication_authority_basis: String,
    pub publication_actor_principal_id: Uuid,
    pub publication_actor_session_id: Uuid,
    pub binding_provenance: Option<KnowledgeBindingProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeContext {
    pub generation: i64,
    pub capability: KnowledgeCapability,
    pub preparation_method: KnowledgeMethodSnapshot,
    pub review_method: KnowledgeMethodSnapshot,
    pub exact_revision: Option<KnowledgeUnitRevision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeItem {
    pub unit_id: Uuid,
    pub revision: i64,
    pub rdf_digest: String,
    pub source_sha256: String,
    pub why_included: String,
    pub unit_iri: String,
    pub revision_iri: String,
    pub source_iri: String,
    pub source_uri: String,
    pub title: String,
    pub statement: String,
    pub modality: KnowledgeModality,
    pub action: String,
    pub target_iri: String,
    pub conditions: Vec<String>,
    pub exceptions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeManifest {
    pub id: Uuid,
    pub digest: String,
    pub semantic_digest: String,
    pub workspace_generation: i64,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub selected: Vec<PipelineKnowledgeItem>,
    pub unresolved_needs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineKnowledgeState {
    Inactive,
    Current,
    Stale,
    NeedsContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeStatus {
    pub state: PipelineKnowledgeState,
    pub current_generation: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_unit_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumedKnowledgeManifestRef {
    pub manifest_id: Uuid,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshPipelineKnowledge {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshPipelineKnowledgeOutcome {
    Refreshed(PipelineKnowledgeManifest),
    Replay(PipelineKnowledgeManifest),
}

#[derive(Debug, Clone)]
pub struct ResolvedKnowledgeBinding {
    pub binding: KnowledgeBinding,
    pub definition: Option<PipelineDefinitionSnapshot>,
}
