use crate::{
    KnowledgeChangePhaseId, KnowledgeKind, KnowledgeLifecycleOperation, KnowledgeProfileId,
    PipelineDeliveryMode, PipelineInstructionSnapshot,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeContractRef {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub source_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeObligationApplicability {
    Always,
    Operation {
        operation: KnowledgeLifecycleOperation,
    },
    Operations {
        operations: Vec<KnowledgeLifecycleOperation>,
    },
    KnowledgeKind {
        knowledge_kind: KnowledgeKind,
    },
    DeclaredCondition {
        condition_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProfileObligationDefinition {
    pub id: String,
    pub requirement: String,
    pub phase_id: KnowledgeChangePhaseId,
    pub applicability: KnowledgeObligationApplicability,
    pub required: bool,
    pub depends_on: Vec<String>,
    pub method_refs: Vec<KnowledgeContractRef>,
    pub shape_refs: Vec<KnowledgeContractRef>,
    pub required_outputs: Vec<String>,
    pub reuse_rule_ref: KnowledgeContractRef,
    pub terminal_rule_refs: Vec<KnowledgeContractRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProfileContract {
    pub profile_id: KnowledgeProfileId,
    pub version: String,
    pub digest: String,
    pub applicable_kinds: Vec<KnowledgeKind>,
    pub operations: Vec<KnowledgeLifecycleOperation>,
    pub inherits: Vec<KnowledgeProfileId>,
    pub compatible_profiles: Vec<KnowledgeProfileId>,
    pub shape_refs: Vec<KnowledgeContractRef>,
    pub methods: Vec<PipelineInstructionSnapshot>,
    pub obligations: Vec<KnowledgeProfileObligationDefinition>,
    pub evidence_rule_refs: Vec<KnowledgeContractRef>,
    pub freshness_rule_refs: Vec<KnowledgeContractRef>,
    pub authority_rule_refs: Vec<KnowledgeContractRef>,
    pub impact_rule_refs: Vec<KnowledgeContractRef>,
    pub retention_rule_refs: Vec<KnowledgeContractRef>,
    pub index_rule_refs: Vec<KnowledgeContractRef>,
    pub terminal_rule_refs: Vec<KnowledgeContractRef>,
    /// Declares that this profile contract covers the complete DK2 lifecycle;
    /// it does not report runtime dependency health or acceptance readiness.
    pub lifecycle_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProfileRegistry {
    pub version: String,
    pub digest: String,
    pub profiles: Vec<KnowledgeProfileContract>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgePhaseExecutor {
    Agent,
    Backend,
    Publisher,
    Worker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgePhaseOutputKind {
    ChangeIntent,
    BaselineManifest,
    BranchPlan,
    EvidenceManifest,
    ProposedChangeset,
    ObligationReceipts,
    ImpactPlan,
    ReviewReceipt,
    ReadyToCommit,
    PublisherReceipt,
    EffectsReport,
    ChangeResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeChangePhaseDefinition {
    pub id: KnowledgeChangePhaseId,
    pub ordinal: u32,
    pub title: String,
    pub executor: KnowledgePhaseExecutor,
    pub output_kind: KnowledgePhaseOutputKind,
    pub depends_on: Vec<KnowledgeChangePhaseId>,
    pub allowed_backward_to: Vec<KnowledgeChangePhaseId>,
    pub instructions: Vec<PipelineInstructionSnapshot>,
    pub methods: Vec<PipelineInstructionSnapshot>,
    pub required_input_refs: Vec<String>,
    pub required_output_refs: Vec<String>,
    pub required_obligation_ids: Vec<String>,
    pub output_contract_ref: KnowledgeContractRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeChangeDefinition {
    pub version: String,
    pub digest: String,
    pub registry_version: String,
    pub registry_digest: String,
    pub overview: PipelineInstructionSnapshot,
    pub default_mode: PipelineDeliveryMode,
    pub allowed_modes: Vec<PipelineDeliveryMode>,
    pub phases: Vec<KnowledgeChangePhaseDefinition>,
    pub completion_contract_ref: KnowledgeContractRef,
    pub escalation_contract_ref: KnowledgeContractRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBranchObligation {
    pub operation_id: Uuid,
    pub profile_id: KnowledgeProfileId,
    pub profile_version: String,
    pub profile_digest: String,
    pub obligation_id: String,
    pub requirement: String,
    pub phase_id: KnowledgeChangePhaseId,
    pub applicability: KnowledgeObligationApplicability,
    pub method_refs: Vec<KnowledgeContractRef>,
    pub shape_refs: Vec<KnowledgeContractRef>,
    pub dependency_obligation_ids: Vec<String>,
    pub pending_qualification: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBranchPlan {
    pub revision: i64,
    pub digest: String,
    pub definition_version: String,
    pub definition_digest: String,
    pub registry_version: String,
    pub registry_digest: String,
    pub delivery_mode: PipelineDeliveryMode,
    pub operation_ids: Vec<Uuid>,
    pub profiles: Vec<KnowledgeProfileId>,
    pub obligations: Vec<KnowledgeBranchObligation>,
    pub policy_refs: Vec<KnowledgeContractRef>,
    pub shape_refs: Vec<KnowledgeContractRef>,
    pub method_refs: Vec<KnowledgeContractRef>,
}
