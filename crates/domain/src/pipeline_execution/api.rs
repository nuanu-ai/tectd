use super::*;
use crate::{PipelineCheckpointRef, PipelineInquiryContract, ResearchCheckpointDraft};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginPipelineRun {
    pub request_id: Uuid,
    pub scope_id: Uuid,
    pub slice_id: Uuid,
    pub slice_revision: i64,
    #[serde(default)]
    pub delivery_mode: Option<PipelineDeliveryMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inquiry: Option<PipelineInquiryContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_checkpoint: Option<PipelineCheckpointRef>,
    pub qualification_reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineRunContextQuery {
    pub run_id: Uuid,
    #[serde(default)]
    pub view: PipelineRunContextView,
    #[serde(default)]
    pub output_id: Option<Uuid>,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRunContextView {
    #[default]
    Current,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineContextResponse {
    Current(Box<PipelineRunContext>),
    Output(Box<PipelinePhaseOutput>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineTerminalResultDraft {
    pub summary: String,
    pub evidence: Vec<SliceResultEvidence>,
    pub scope_impact: String,
    pub remaining_work: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletePipelinePhase {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub outcome: PipelinePhaseOutcome,
    pub transition: PipelineTransition,
    pub output: PipelinePhaseOutputDraft,
    pub consumed_outputs: Vec<PipelineConsumedOutput>,
    pub consumed_inputs: Vec<PipelineConsumedInput>,
    #[serde(default)]
    pub revisit_phase_id: Option<String>,
    #[serde(default)]
    pub escalation_target: Option<PipelineKind>,
    #[serde(default)]
    pub terminal_result: Option<PipelineTerminalResultDraft>,
    #[serde(default)]
    pub publish_blocked_result: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_knowledge: Option<ConsumedKnowledgeManifestRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub research_checkpoint: Option<ResearchCheckpointDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordPipelineInput {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EscalatePipelineDelivery {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineMutationOutcome {
    pub context: PipelineRunContext,
    pub result: Option<SliceResult>,
}
