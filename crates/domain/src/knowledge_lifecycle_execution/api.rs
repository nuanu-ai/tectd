use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeChangeContext {
    pub change_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<KnowledgeChangeOrigin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub maintenance_tasks: Vec<crate::KnowledgeMaintenanceTask>,
    pub run: KnowledgeChangeRun,
    pub definition: crate::KnowledgeChangeDefinition,
    pub delivered_phases: Vec<crate::KnowledgeChangePhaseDefinition>,
    pub attempts: Vec<KnowledgePhaseAttempt>,
    pub outputs: Vec<KnowledgePhaseOutput>,
    pub inputs: Vec<KnowledgeChangeInput>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub erased_payloads: Vec<KnowledgePayloadTombstone>,
    pub baseline: Option<KnowledgeBaselineManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_baseline: Option<KnowledgeBaselineManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_source_pin_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_impact: Option<KnowledgeImpactPlan>,
    pub plan: Option<crate::KnowledgeBranchPlan>,
    pub ready_to_commit: Option<KnowledgeReadyToCommit>,
    pub publisher_receipt: Option<KnowledgePublisherReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erased_publisher_receipt: Option<KnowledgeErasedPublisherReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub erased_no_change_proof: Option<crate::KnowledgeErasedNoChangeProof>,
    pub effects_report: Option<KnowledgeEffectsReport>,
    pub result: Option<KnowledgeChangeResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeginKnowledgeChangeOutcome {
    Created(Box<KnowledgeChangeContext>),
    Replay(Box<KnowledgeChangeContext>),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeLifecycleView {
    #[default]
    Current,
    History,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeLifecycleQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_id: Option<Uuid>,
    #[serde(default)]
    pub view: KnowledgeLifecycleView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<KnowledgeLifecycleFragmentQuery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeLifecycleFragmentQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_digest: Option<String>,
    pub offset: u64,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeLifecycleSummary {
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub status: PipelineRunStatus,
    pub current_phase_id: Option<KnowledgeChangePhaseId>,
    pub operation_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeLifecycleOverview {
    pub workspace_generation: i64,
    pub active: Vec<KnowledgeLifecycleSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeLifecycleResponse {
    Overview(KnowledgeLifecycleOverview),
    Current(Box<KnowledgeChangeContext>),
    History(Vec<KnowledgePhaseAttempt>),
    Output(Box<KnowledgePhaseOutput>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeDocumentRevision {
    pub unit_id: Uuid,
    pub revision: i64,
    pub lifecycle: KnowledgeLifecycleState,
    pub document: KnowledgeDocumentDraft,
    pub source_digests: Vec<String>,
    pub rdf_digest: String,
    pub unit_iri: String,
    pub revision_iri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeUnitTombstone {
    pub unit_id: Uuid,
    pub lifecycle: KnowledgeLifecycleState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeUnitResponse {
    LegacyConstraint(Box<crate::KnowledgeUnitRevision>),
    Document(Box<KnowledgeDocumentRevision>),
    PayloadErased(KnowledgeUnitTombstone),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeUnitQuery {
    pub unit_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<KnowledgeLifecycleFragmentQuery>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteKnowledgeChangePhase {
    pub request_id: Uuid,
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: KnowledgeChangePhaseId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<KnowledgeAgentPhaseOutputDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revisit_phase_id: Option<KnowledgeChangePhaseId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordKnowledgeChangeInput {
    pub request_id: Uuid,
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub revisit_phase_id: KnowledgeChangePhaseId,
    pub reason: String,
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis_amendment: Option<KnowledgeBasisAmendment>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitKnowledgeChange {
    pub request_id: Uuid,
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub seal_id: Uuid,
    pub plan_revision: i64,
    pub plan_digest: String,
    pub sealed_command_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettleKnowledgeChangeEffects {
    pub request_id: Uuid,
    pub change_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub publisher_receipt_id: Uuid,
    #[serde(default)]
    pub effect_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeChangeMutationOutcome {
    Advanced(Box<KnowledgeChangeContext>),
    Replay(Box<KnowledgeChangeContext>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommitKnowledgeChangeOutcome {
    Applied(KnowledgePublisherReceipt),
    AppliedErased(KnowledgeErasedPublisherReceipt),
    Replay(KnowledgePublisherReceipt),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettleKnowledgeChangeEffectsOutcome {
    Settled(KnowledgeEffectsReport),
    Replay(KnowledgeEffectsReport),
}
