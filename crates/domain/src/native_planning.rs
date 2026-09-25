use crate::{CandidateBoundary, CandidateMethodSnapshot, CandidateRuleSnapshot};
use crate::{PipelineCatalogueSnapshot, PipelineKind, PipelineRunStatus};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeScope {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub revision: i64,
    pub source_candidate_set_id: Uuid,
    pub source_candidate_set_revision: i64,
    pub source_snapshot_id: Uuid,
    pub source_candidate_id: Uuid,
    pub source_candidate_revision: i64,
    pub boundary: CandidateBoundary,
    pub title: String,
    pub outcome: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub slice_candidate_set_id: Uuid,
    pub slice_input_cursor: i64,
    pub slice_latest_input: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenScopeContext {
    pub scope: NativeScope,
    pub planning: SliceCandidateContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenScopeOutcome {
    Created(OpenScopeContext),
    Replay(OpenScopeContext),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenScope {
    pub request_id: Uuid,
    pub candidate_set_id: Uuid,
    pub candidate_set_revision: i64,
    pub candidate_snapshot_id: Uuid,
    pub candidate_id: Uuid,
    pub candidate_revision: i64,
    #[serde(default)]
    pub task_context: crate::PlanningTaskContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_knowledge: Option<crate::PlanningManifestGuard>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeOpenBasis {
    pub boundary: CandidateBoundary,
    pub title: String,
    pub outcome: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
    pub source_candidate_set_revision: i64,
    pub source_snapshot_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlicePlanningSnapshot {
    pub id: Uuid,
    pub sequence: i64,
    pub scope_revision: i64,
    pub source_candidate_set_revision: i64,
    pub source_snapshot_id: Uuid,
    pub planning_latest_input: i64,
    pub method: CandidateMethodSnapshot,
    pub registry_revision: String,
    pub registry_digest: String,
    pub rules: Vec<CandidateRuleSnapshot>,
    pub catalogue: PipelineCatalogueSnapshot,
    pub result_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlicePlanningSnapshotMaterial {
    pub method: CandidateMethodSnapshot,
    pub registry_revision: String,
    pub registry_digest: String,
    pub rules: Vec<CandidateRuleSnapshot>,
    pub catalogue: PipelineCatalogueSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceCandidateSetStatus {
    Draft,
    ReviewRequired,
    Ready,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceCandidateSet {
    pub id: Uuid,
    pub scope_id: Uuid,
    pub revision: i64,
    pub status: SliceCandidateSetStatus,
    pub current_snapshot_id: Uuid,
    pub input_cursor: i64,
    pub latest_input: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlicePlanningInput {
    pub id: Uuid,
    pub sequence: i64,
    pub source_result_id: Option<Uuid>,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceCandidateHistoryStatus {
    Active,
    Prior,
    Superseded,
    Opened,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceCandidateHistoryEntry {
    pub candidate_id: Uuid,
    pub candidate_revision: i64,
    pub title: String,
    pub status: SliceCandidateHistoryStatus,
    pub superseded_reason: Option<String>,
    pub replacement_candidate_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceDraftIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SliceCandidateRef {
    Local { local: String },
    Existing { candidate_id: Uuid, revision: i64 },
}

/// Caller-authored route constraints attached to one saved Work revision.
/// Each present field is an assertion, not independently verified evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelRouteCallerFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_budget_units: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub available_latency_ms: Option<u64>,
}

impl ModelRouteCallerFacts {
    pub fn validate(&self) -> crate::Result<()> {
        if self.role.is_none()
            && self.tool.is_none()
            && self.data_class.is_none()
            && self.remaining_budget_units.is_none()
            && self.available_latency_ms.is_none()
        {
            return Err(crate::Error::InvalidArguments);
        }
        for value in [&self.role, &self.tool, &self.data_class]
            .into_iter()
            .flatten()
        {
            if value.is_empty()
                || value.len() > 128
                || !value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
                })
            {
                return Err(crate::Error::InvalidArguments);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SliceCandidateDraftNode {
    Work {
        identity: SliceDraftIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_route_facts: Option<ModelRouteCallerFacts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        change_rationale: Option<String>,
        title: String,
        outcome: String,
        #[serde(default)]
        includes: Vec<String>,
        #[serde(default)]
        excludes: Vec<String>,
        #[serde(default)]
        dependencies: Vec<SliceCandidateRef>,
        proof: Vec<String>,
        pipeline: PipelineKind,
        pipeline_reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why_lightweight_insufficient: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        why_further_vertical_split_not_viable: Option<String>,
        #[serde(default)]
        source_result_ids: Vec<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_checkpoint: Option<crate::PipelineCheckpointRef>,
    },
    Decision {
        identity: SliceDraftIdentity,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        change_rationale: Option<String>,
        title: String,
        question: String,
        resolution_criteria: Vec<String>,
        #[serde(default)]
        dependencies: Vec<SliceCandidateRef>,
        #[serde(default)]
        source_result_ids: Vec<Uuid>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceCandidateSupersessionDraft {
    pub candidate_id: Uuid,
    pub revision: i64,
    pub reason: String,
    #[serde(default)]
    pub source_result_ids: Vec<Uuid>,
    #[serde(default)]
    pub replacements: Vec<SliceCandidateRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceCandidateDraft {
    pub coverage_summary: String,
    pub nodes: Vec<SliceCandidateDraftNode>,
    #[serde(default)]
    pub supersessions: Vec<SliceCandidateSupersessionDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SliceCandidateNode {
    Work {
        id: Uuid,
        revision: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_route_facts: Option<ModelRouteCallerFacts>,
        title: String,
        outcome: String,
        includes: Vec<String>,
        excludes: Vec<String>,
        dependencies: Vec<Uuid>,
        proof: Vec<String>,
        pipeline: PipelineKind,
        pipeline_reason: String,
        why_lightweight_insufficient: Option<String>,
        why_further_vertical_split_not_viable: Option<String>,
        source_result_ids: Vec<Uuid>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source_checkpoint: Option<crate::PipelineCheckpointRef>,
    },
    Decision {
        id: Uuid,
        revision: i64,
        title: String,
        question: String,
        resolution_criteria: Vec<String>,
        dependencies: Vec<Uuid>,
        source_result_ids: Vec<Uuid>,
    },
}

impl SliceCandidateNode {
    pub fn id(&self) -> Uuid {
        match self {
            Self::Work { id, .. } | Self::Decision { id, .. } => *id,
        }
    }
    pub fn revision(&self) -> i64 {
        match self {
            Self::Work { revision, .. } | Self::Decision { revision, .. } => *revision,
        }
    }
    pub fn title(&self) -> &str {
        match self {
            Self::Work { title, .. } | Self::Decision { title, .. } => title,
        }
    }
    pub fn dependencies(&self) -> &[Uuid] {
        match self {
            Self::Work { dependencies, .. } | Self::Decision { dependencies, .. } => dependencies,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSliceCandidateDraft {
    pub coverage_summary: String,
    pub nodes: Vec<SliceCandidateNode>,
    pub supersessions: Vec<ResolvedSliceCandidateSupersession>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSliceCandidateSupersession {
    pub candidate_id: Uuid,
    pub revision: i64,
    pub reason: String,
    pub source_result_ids: Vec<Uuid>,
    pub replacement_candidate_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlicePlanReviewVerdict {
    Ready,
    Revise,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlicePlanFinding {
    pub material: bool,
    pub summary: String,
    #[serde(default)]
    pub candidate_ids: Vec<Uuid>,
    pub disposition: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceCandidateReviewDraft {
    pub verdict: SlicePlanReviewVerdict,
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<SlicePlanFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceCandidateReview {
    pub revision: i64,
    pub verdict: SlicePlanReviewVerdict,
    pub summary: String,
    pub findings: Vec<SlicePlanFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SliceCandidateContext {
    pub scope: NativeScope,
    pub candidate_set: SliceCandidateSet,
    pub snapshot: SlicePlanningSnapshot,
    pub draft: Option<ResolvedSliceCandidateDraft>,
    pub reviews: Vec<SliceCandidateReview>,
    pub inputs: Vec<SlicePlanningInput>,
    pub history: Vec<SliceCandidateHistoryEntry>,
    pub slices: Vec<NativeSlice>,
    pub results: Vec<SliceResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<crate::PipelineResearchCheckpoint>,
    pub stale_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_knowledge: Option<crate::PlanningKnowledgeStatus>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceCandidateContextView {
    Overview,
    Inputs,
    Candidates,
    Reviews,
    History,
    Results,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceCandidateContextQuery {
    pub scope_id: Uuid,
    pub view: SliceCandidateContextView,
    #[serde(default)]
    pub after: Option<i64>,
    pub limit: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveSliceCandidateDraft {
    pub scope_id: Uuid,
    pub candidate_set_id: Uuid,
    pub revision: i64,
    pub snapshot_id: Uuid,
    pub input_cursor: i64,
    pub request_id: Uuid,
    pub draft: SliceCandidateDraft,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_knowledge: Option<crate::PlanningManifestGuard>,
    /// Explicit provenance for a caller-authored save after a Matrix selection.
    /// A Matrix disposition alone never creates this draft or its receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix_selection: Option<crate::MatrixPlanningSelection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewSliceCandidateSet {
    pub scope_id: Uuid,
    pub candidate_set_id: Uuid,
    pub revision: i64,
    pub snapshot_id: Uuid,
    pub input_cursor: i64,
    pub request_id: Uuid,
    pub review: SliceCandidateReviewDraft,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_knowledge: Option<crate::PlanningManifestGuard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordSliceCandidateInput {
    pub scope_id: Uuid,
    pub candidate_set_id: Uuid,
    pub revision: i64,
    pub request_id: Uuid,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefreshSliceCandidateSet {
    pub scope_id: Uuid,
    pub candidate_set_id: Uuid,
    pub revision: i64,
    pub request_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_context: Option<crate::PlanningTaskContext>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SliceState {
    Open,
    Completed,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSlice {
    pub id: Uuid,
    pub scope_id: Uuid,
    pub revision: i64,
    pub candidate_id: Uuid,
    pub candidate_revision: i64,
    pub opening_snapshot_id: Uuid,
    pub title: String,
    pub outcome: String,
    pub pipeline: PipelineKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_option_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_source_definition_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_plan_source_definition_digest: Option<String>,
    pub state: SliceState,
    pub pipeline_status: String,
    #[serde(default)]
    pub pipeline_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_change_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge_status: Option<PipelineRunStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_checkpoint: Option<crate::PipelineCheckpointRef>,
    pub execution_claimed: bool,
}

mod verification_plan_binding;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenSliceOutcome {
    Created(NativeSlice),
    Replay(NativeSlice),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenSlice {
    pub request_id: Uuid,
    pub scope_id: Uuid,
    pub scope_revision: i64,
    pub candidate_set_id: Uuid,
    pub candidate_set_revision: i64,
    pub candidate_snapshot_id: Uuid,
    pub candidate_id: Uuid,
    pub candidate_revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition_id: Option<Uuid>,
}

mod result;
pub use result::*;

#[cfg(test)]
mod model_route_caller_facts_tests {
    use super::*;

    #[test]
    fn old_work_payload_omits_facts_and_typed_fields_round_trip() {
        let legacy = SliceCandidateNode::Work {
            id: Uuid::new_v4(),
            revision: 1,
            model_route_facts: None,
            title: "Work".into(),
            outcome: "Outcome".into(),
            includes: vec![],
            excludes: vec![],
            dependencies: vec![],
            proof: vec!["Proof".into()],
            pipeline: PipelineKind::LightweightTddDevelopment,
            pipeline_reason: "Small".into(),
            why_lightweight_insufficient: None,
            why_further_vertical_split_not_viable: None,
            source_result_ids: vec![],
            source_checkpoint: None,
        };
        let encoded = serde_json::to_value(&legacy).unwrap();
        assert!(encoded.get("model_route_facts").is_none());
        assert_eq!(
            serde_json::from_value::<SliceCandidateNode>(encoded).unwrap(),
            legacy
        );

        let mut typed = legacy;
        if let SliceCandidateNode::Work {
            model_route_facts, ..
        } = &mut typed
        {
            *model_route_facts = Some(ModelRouteCallerFacts {
                role: Some("agent".into()),
                tool: Some("code".into()),
                data_class: Some("internal".into()),
                remaining_budget_units: Some(10),
                available_latency_ms: Some(50),
            });
        }
        let encoded = serde_json::to_value(&typed).unwrap();
        assert_eq!(encoded["model_route_facts"]["role"], "agent");
        assert_eq!(
            serde_json::from_value::<SliceCandidateNode>(encoded).unwrap(),
            typed
        );
    }

    #[test]
    fn caller_facts_reject_empty_or_unusable_values() {
        assert!(ModelRouteCallerFacts::default().validate().is_err());
        assert!(
            ModelRouteCallerFacts {
                role: Some(" agent".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ModelRouteCallerFacts {
                role: Some("agent".into()),
                ..Default::default()
            }
            .validate()
            .is_ok()
        );
        let unknown = serde_json::json!({"role":"agent", "host_capabilities":["gpu"]});
        assert!(serde_json::from_value::<ModelRouteCallerFacts>(unknown).is_err());
    }
}
