use crate::{
    PipelineKind, SliceResult, SliceResultEvidence,
    pipeline_followups::{PipelineFollowupContract, PipelineFollowupProposal},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const MAX_PIPELINE_OUTPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PIPELINE_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_PIPELINE_CONTEXT_ID_BYTES: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineDeliveryMode {
    Whole,
    Phasewise,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineRunStatus {
    Active,
    WaitingInput,
    Blocked,
    Completed,
    Escalated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelinePhaseOutcome {
    Completed,
    WaitingInput,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineTransition {
    Continue,
    Complete,
    Block,
    Escalate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelinePhaseRetryPolicy {
    Repeatable,
    ExactReplayOnly,
    ReconciliationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineOutputConstraint {
    FieldEquals {
        field: String,
        value: String,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldNotEquals {
        field: String,
        value: String,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldIntegerEquals {
        field: String,
        value: i64,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldIntegerNotEquals {
        field: String,
        value: i64,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldIntegerMinimum {
        field: String,
        value: i64,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldBooleanEquals {
        field: String,
        value: bool,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldOneOf {
        field: String,
        values: Vec<String>,
        #[serde(default)]
        when_verdict: Option<String>,
    },
    FieldsEqual {
        field: String,
        other_field: String,
        #[serde(default)]
        when_verdict: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineVerdictRoute {
    pub verdict: String,
    pub outcome: PipelinePhaseOutcome,
    pub transition: PipelineTransition,
    pub dispositions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub revisit_to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineInstructionSnapshot {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub body: String,
    pub origin_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineArtifactRequirement {
    pub name_pattern: String,
    pub media_type: String,
    #[serde(default)]
    pub schema_ref: Option<String>,
    #[serde(default)]
    pub schema_resource_id: Option<String>,
    #[serde(default)]
    pub schema_resource_digest: Option<String>,
    pub required: bool,
    pub minimum_matches: u32,
    #[serde(default)]
    pub when_verdict: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelinePhaseArtifactDraft {
    pub name: String,
    pub media_type: String,
    pub body: String,
    pub digest: String,
    #[serde(default)]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineArtifactDigestRef {
    pub name: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineValidatorContract {
    pub resource_id: String,
    pub version: String,
    pub digest: String,
    pub stage: String,
    pub artifact_patterns: Vec<String>,
    #[serde(default)]
    pub required_verdicts: Vec<String>,
    pub success_verdicts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineValidatorReceipt {
    pub resource_id: String,
    pub version: String,
    pub digest: String,
    pub stage: String,
    pub command: String,
    pub exit_code: i64,
    pub valid: bool,
    pub artifacts: Vec<PipelineArtifactDigestRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelinePhaseDefinition {
    pub id: String,
    pub ordinal: u32,
    pub title: String,
    pub required: bool,
    pub disposition_required: bool,
    pub instructions: Vec<PipelineInstructionSnapshot>,
    pub skills: Vec<PipelineInstructionSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<PipelineInstructionSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_artifacts: Vec<PipelineArtifactRequirement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validator_contracts: Vec<PipelineValidatorContract>,
    pub required_fields: Vec<String>,
    pub allowed_verdicts: Vec<String>,
    pub required_dispositions: Vec<String>,
    #[serde(default)]
    pub allowed_dispositions: Vec<String>,
    #[serde(default)]
    pub output_constraints: Vec<PipelineOutputConstraint>,
    #[serde(default)]
    pub verdict_routes: Vec<PipelineVerdictRoute>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub followup_contracts: Vec<PipelineFollowupContract>,
    pub allowed_backward_to: Vec<String>,
    pub fresh_reviewer_input: bool,
    pub retry_policy: PipelinePhaseRetryPolicy,
    pub output_contract: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineDefinitionSnapshot {
    pub kind: PipelineKind,
    pub version: String,
    pub digest: String,
    pub overview: PipelineInstructionSnapshot,
    pub default_mode: PipelineDeliveryMode,
    pub allowed_modes: Vec<PipelineDeliveryMode>,
    pub phases: Vec<PipelinePhaseDefinition>,
    pub completion_contract: String,
    pub escalation_contract: String,
    pub forbidden_claims: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: Uuid,
    pub scope_id: Uuid,
    pub slice_id: Uuid,
    pub slice_revision: i64,
    pub revision: i64,
    pub definition_kind: PipelineKind,
    pub definition_version: String,
    pub definition_digest: String,
    pub delivery_mode: PipelineDeliveryMode,
    pub qualification_reason: String,
    pub status: PipelineRunStatus,
    pub current_phase_id: Option<String>,
    pub current_phase_ordinal: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineSkillReadReceipt {
    pub instruction_id: String,
    pub version: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineReviewerAttestation {
    pub reviewer_identity: String,
    pub reviewer_context_id: String,
    pub producer_context_ids: Vec<String>,
    pub fresh_input: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct PipelineConsumedOutput {
    pub phase_id: String,
    pub output_revision: i64,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
pub struct PipelineConsumedInput {
    pub input_id: Uuid,
    pub sequence: i64,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelinePhaseOutputDraft {
    pub body: String,
    pub producer_context_id: String,
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    #[serde(default)]
    pub verdict: Option<String>,
    #[serde(default)]
    pub dispositions: Vec<String>,
    #[serde(default)]
    pub skill_reads: Vec<PipelineSkillReadReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_reads: Vec<PipelineSkillReadReceipt>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<PipelinePhaseArtifactDraft>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub validator_receipts: Vec<PipelineValidatorReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub followup_proposal: Option<PipelineFollowupProposal>,
    #[serde(default)]
    pub reviewer_context: Option<PipelineReviewerAttestation>,
    #[serde(default)]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelinePhaseAttempt {
    pub id: Uuid,
    pub run_id: Uuid,
    pub phase_id: String,
    pub phase_ordinal: u32,
    pub attempt: i64,
    pub outcome: PipelinePhaseOutcome,
    pub transition: PipelineTransition,
    pub output_revision: i64,
    pub output_id: Uuid,
    pub output_digest: String,
    pub output_reference: Option<String>,
    pub actor_session_id: Uuid,
    pub reviewer_context: Option<PipelineReviewerAttestation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineOutputBinding {
    pub phase_id: String,
    pub phase_ordinal: u32,
    pub output_revision: i64,
    pub output_id: Uuid,
    pub output_digest: String,
    pub reference: Option<String>,
    pub stale: bool,
    pub stale_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelinePhaseOutput {
    pub id: Uuid,
    pub run_id: Uuid,
    pub phase_id: String,
    pub phase_ordinal: u32,
    pub revision: i64,
    pub body: String,
    pub producer_context_id: String,
    pub digest: String,
    pub reference: Option<String>,
    pub fields: BTreeMap<String, String>,
    pub verdict: Option<String>,
    pub dispositions: Vec<String>,
    pub skill_reads: Vec<PipelineSkillReadReceipt>,
    pub resource_reads: Vec<PipelineSkillReadReceipt>,
    pub artifacts: Vec<PipelinePhaseArtifactDraft>,
    pub validator_receipts: Vec<PipelineValidatorReceipt>,
    pub followup_proposal: Option<PipelineFollowupProposal>,
    pub stale: bool,
    pub stale_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineInput {
    pub id: Uuid,
    pub sequence: i64,
    pub phase_id: String,
    pub input: String,
    pub digest: String,
    pub actor_session_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineRunContext {
    pub run: PipelineRun,
    pub definition: PipelineDefinitionSnapshot,
    pub delivered_phases: Vec<PipelinePhaseDefinition>,
    pub attempts: Vec<PipelinePhaseAttempt>,
    pub bindings: Vec<PipelineOutputBinding>,
    pub outputs: Vec<PipelinePhaseOutput>,
    pub outputs_complete: bool,
    pub inputs: Vec<PipelineInput>,
    pub result: Option<SliceResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BeginPipelineRunOutcome {
    Created(PipelineRunContext),
    Replay(PipelineRunContext),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginPipelineRun {
    pub request_id: Uuid,
    pub scope_id: Uuid,
    pub slice_id: Uuid,
    pub slice_revision: i64,
    #[serde(default)]
    pub delivery_mode: Option<PipelineDeliveryMode>,
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
