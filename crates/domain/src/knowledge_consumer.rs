use crate::{
    KnowledgeAccessScope, KnowledgeBindingPurpose, KnowledgeBindingTarget, KnowledgeBindingVersion,
    KnowledgeContractRef, KnowledgeEpistemicState, KnowledgeEvidenceKind, KnowledgeKind,
    KnowledgeLifecycleState, KnowledgeProfileId, KnowledgeProfileSections,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeSourcePin {
    pub source_iri: String,
    pub digest: String,
    pub evidence_kind: KnowledgeEvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    pub evidence_scope: String,
    pub title: String,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeValidationPin {
    pub event_id: Uuid,
    pub event_iri: String,
    pub event_digest: String,
    pub sequence: i64,
    pub source_pins: Vec<PipelineKnowledgeSourcePin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_due_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeBindingPin {
    pub binding_iri: String,
    pub target: KnowledgeBindingTarget,
    pub purpose: KnowledgeBindingPurpose,
    pub version_resolution: KnowledgeBindingVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeResource {
    pub unit_id: Uuid,
    pub revision: i64,
    pub lifecycle: KnowledgeLifecycleState,
    pub access_scope: KnowledgeAccessScope,
    pub rdf_digest: String,
    pub unit_iri: String,
    pub revision_iri: String,
    pub title: String,
    pub canonical_text: String,
    pub knowledge_kind: KnowledgeKind,
    pub epistemic_state: KnowledgeEpistemicState,
    pub target_iris: Vec<String>,
    pub profiles: Vec<KnowledgeProfileId>,
    pub conditions: Vec<String>,
    pub exceptions: Vec<String>,
    pub sections: KnowledgeProfileSections,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inquiry_briefs: Option<Vec<crate::PlanningBrief>>,
    pub source_pins: Vec<PipelineKnowledgeSourcePin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_validation: Option<PipelineKnowledgeValidationPin>,
    pub binding: PipelineKnowledgeBindingPin,
    pub why_included: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineKnowledgeProjectionPolicy {
    FullResources,
    ProgramPlanningBriefs,
    ScopePlanningBriefs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeResourceManifest {
    pub id: Uuid,
    pub digest: String,
    pub semantic_digest: String,
    pub workspace_generation: i64,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub definition_version: String,
    pub definition_digest: String,
    pub method_requirements: Vec<KnowledgeContractRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inquiry: Option<crate::PipelineInquiryContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_policy: Option<PipelineKnowledgeProjectionPolicy>,
    pub selected: Vec<PipelineKnowledgeResource>,
    pub unresolved_needs: Vec<String>,
    pub freshness_warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineKnowledgeResourceState {
    Inactive,
    Current,
    Stale,
    NeedsContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeResourceStatus {
    pub state: PipelineKnowledgeResourceState,
    pub current_generation: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_unit_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub freshness_warnings: Vec<String>,
    pub access_changed: bool,
}
