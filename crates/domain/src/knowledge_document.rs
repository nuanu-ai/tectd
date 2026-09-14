use crate::PipelineArtifactDigestRef;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DK2_MAX_DOCUMENT_BYTES: usize = 512 * 1024;
pub const DK2_MAX_SOURCE_BYTES: usize = 256 * 1024;
pub const DK2_MAX_LIST_ITEMS: usize = 128;
pub const DK2_CONTRACT_VERSION: &str = "0.2.0-dk2.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeKind {
    Constraint,
    Claim,
    Decision,
    Hypothesis,
    Procedure,
    Protocol,
    Infrastructure,
    OperatingModel,
    ProductResearch,
    Security,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeProfileId {
    General,
    Runbook,
    Protocol,
    Devops,
    Operations,
    ProductResearch,
    Security,
}

impl KnowledgeProfileId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Runbook => "runbook",
            Self::Protocol => "protocol",
            Self::Devops => "devops",
            Self::Operations => "operations",
            Self::ProductResearch => "product_research",
            Self::Security => "security",
        }
    }

    pub const fn method_id(self) -> &'static str {
        match self {
            Self::General => "tect:knowledge-profile:general",
            Self::Runbook => "tect:knowledge-profile:runbook",
            Self::Protocol => "tect:knowledge-profile:protocol",
            Self::Devops => "tect:knowledge-profile:devops",
            Self::Operations => "tect:knowledge-profile:operations",
            Self::ProductResearch => "tect:knowledge-profile:product_research",
            Self::Security => "tect:knowledge-profile:security",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeEpistemicState {
    Normative,
    Decision,
    Declared,
    Observed,
    Hypothesis,
    NegativeKnowledge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeEvidenceKind {
    Document,
    Declaration,
    Observation,
    DecisionRecord,
    Research,
    StaticVerification,
    RuntimeVerification,
    NegativeEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeAccessScope {
    WorkspaceMembers,
    OwnersOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSensitivity {
    Public,
    Internal,
    Sensitive,
    Restricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSavedSourceSnapshot {
    pub title: String,
    pub uri: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    pub evidence_kind: KnowledgeEvidenceKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePipelineOutputRef {
    pub run_id: Uuid,
    pub output_id: Uuid,
    pub digest: String,
    pub evidence_kind: KnowledgeEvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    pub evidence_scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<PipelineArtifactDigestRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeSourceRef {
    Snapshot {
        snapshot: KnowledgeSavedSourceSnapshot,
    },
    PipelineOutput {
        output: KnowledgePipelineOutputRef,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeBindingPurpose {
    Required,
    Reference,
    Procedure,
    ProofBasis,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeBindingTarget {
    Workspace,
    Program {
        program_id: Uuid,
    },
    Scope {
        scope_id: Uuid,
    },
    Slice {
        scope_id: Uuid,
        slice_id: Uuid,
    },
    SlicePhase {
        scope_id: Uuid,
        slice_id: Uuid,
        phase_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeBindingVersion {
    CurrentAccepted,
    PinnedRevision { revision: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDocumentBinding {
    pub target: KnowledgeBindingTarget,
    pub purpose: KnowledgeBindingPurpose,
    pub version_resolution: KnowledgeBindingVersion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeProofStatus {
    Documented,
    StaticVerified,
    RuntimeVerified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeConstraintSection {
    pub modality: crate::KnowledgeModality,
    pub action: String,
    pub target_iri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeGeneralSection {
    pub statement: String,
    pub assumptions: Vec<String>,
    pub evidence_scope: String,
    pub rationale: String,
    pub alternatives: Vec<String>,
    pub negative_limits: Vec<String>,
    pub unknown_limits: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeParameterDefinition {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeRunbookStep {
    pub ordinal: u32,
    pub action: String,
    pub expected_result: String,
    pub verification: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeRunbookSection {
    pub purpose_and_fit: String,
    pub target_environment_iris: Vec<String>,
    pub parameters: Vec<KnowledgeParameterDefinition>,
    pub prerequisites: Vec<String>,
    pub required_authority: String,
    pub steps: Vec<KnowledgeRunbookStep>,
    pub failure_and_recovery: String,
    pub proof_status: KnowledgeProofStatus,
    pub proof_evidence_refs: Vec<u32>,
    pub dependency_iris: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProtocolAssertion {
    pub statement: String,
    pub observed: bool,
    pub evidence_refs: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProtocolSection {
    pub specification_uri: String,
    pub specification_version: String,
    pub provider_scope: Vec<String>,
    pub network_scope: Vec<String>,
    pub assertions: Vec<KnowledgeProtocolAssertion>,
    pub capabilities: Vec<String>,
    pub compatibility_constraints: Vec<String>,
    pub negative_states_and_quirks: Vec<String>,
    pub observation_bounds: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDatedObservation {
    pub observed_at: String,
    pub status: String,
    pub limits: Vec<String>,
    pub evidence_refs: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDevopsSection {
    pub asset_iris: Vec<String>,
    pub environment_iris: Vec<String>,
    pub topology_links: Vec<String>,
    pub ownership: Vec<String>,
    pub configuration_refs: Vec<String>,
    pub observations: Vec<KnowledgeDatedObservation>,
    pub deployment_surfaces: Vec<String>,
    pub configuration_custody: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeRoleResponsibility {
    pub role: String,
    pub responsibilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeMetricDefinition {
    pub name: String,
    pub meaning: String,
    pub objective: String,
    pub signal_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeOperationsSection {
    pub operating_purpose: String,
    pub roles: Vec<KnowledgeRoleResponsibility>,
    pub cadence: String,
    pub handoffs: Vec<String>,
    pub escalation: Vec<String>,
    pub status_semantics: Vec<String>,
    pub metrics: Vec<KnowledgeMetricDefinition>,
    pub signal_sources: Vec<String>,
    pub exceptions: Vec<String>,
    pub ownership_gaps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeResearchEvidence {
    pub claim: String,
    pub source_refs: Vec<u32>,
    pub synthetic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProductResearchSection {
    pub question: String,
    pub evidence_map: Vec<KnowledgeResearchEvidence>,
    pub assumptions: Vec<String>,
    pub segments: Vec<String>,
    pub alternatives: Vec<String>,
    pub conclusions_and_decisions: Vec<String>,
    pub observation_limits: Vec<String>,
    pub negative_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSecuritySection {
    pub asset_iris: Vec<String>,
    pub trust_boundaries: Vec<String>,
    pub threats: Vec<String>,
    pub controls: Vec<String>,
    pub evidence_refs: Vec<u32>,
    pub verification_status: String,
    pub sensitivity: KnowledgeSensitivity,
    pub applicable_authority: String,
    pub exceptions: Vec<String>,
    pub finding_state: String,
    pub remediation_proof_refs: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeProfileSections {
    pub constraint: Option<KnowledgeConstraintSection>,
    pub general: Option<KnowledgeGeneralSection>,
    pub runbook: Option<KnowledgeRunbookSection>,
    pub protocol: Option<KnowledgeProtocolSection>,
    pub devops: Option<KnowledgeDevopsSection>,
    pub operations: Option<KnowledgeOperationsSection>,
    pub product_research: Option<KnowledgeProductResearchSection>,
    pub security: Option<KnowledgeSecuritySection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeDocumentDraft {
    pub title: String,
    pub canonical_text: String,
    pub knowledge_kind: KnowledgeKind,
    pub epistemic_state: KnowledgeEpistemicState,
    pub target_iris: Vec<String>,
    pub conditions: Vec<String>,
    pub exceptions: Vec<String>,
    pub sources: Vec<KnowledgeSourceRef>,
    pub bindings: Vec<KnowledgeDocumentBinding>,
    pub profiles: Vec<KnowledgeProfileId>,
    pub access_scope: KnowledgeAccessScope,
    pub owner_ref: String,
    pub authority_basis: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_due_at: Option<String>,
    pub sections: KnowledgeProfileSections,
}
