use crate::{PipelineInquiryContract, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCheckpointBasis {
    pub consumed_outputs: Vec<crate::PipelineConsumedOutput>,
    pub consumed_inputs: Vec<crate::PipelineConsumedInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumed_knowledge: Option<crate::ConsumedKnowledgeManifestRef>,
}

pub const MAX_CHECKPOINT_TEXT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCheckpointRef {
    pub checkpoint_id: Uuid,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResearchCheckpointDraft {
    pub question: String,
    pub answer_criteria: String,
    pub inquiry: PipelineInquiryContract,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineCheckpointStatus {
    Open,
    Accepted,
    Rejected,
    Cancelled,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineResearchCheckpoint {
    pub checkpoint: PipelineCheckpointRef,
    pub status: PipelineCheckpointStatus,
    pub producer_run_id: Uuid,
    pub producer_run_revision: i64,
    pub producer_definition_version: String,
    pub producer_definition_digest: String,
    pub producer_phase_id: String,
    pub producer_output_id: Uuid,
    pub producer_output_revision: i64,
    pub producer_output_digest: String,
    pub basis: PipelineCheckpointBasis,
    pub question: String,
    pub answer_criteria: String,
    pub inquiry: PipelineInquiryContract,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_run_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_result_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_terminal_output_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consumer_terminal_output_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_action: Option<ResolvePipelineCheckpointAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvePipelineCheckpointAction {
    Accept,
    Reject,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineCheckpointTerminalRef {
    pub result_id: Uuid,
    pub output_id: Uuid,
    pub output_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvePipelineCheckpoint {
    pub request_id: Uuid,
    pub producer_run_id: Uuid,
    pub producer_run_revision: i64,
    pub checkpoint: PipelineCheckpointRef,
    pub action: ResolvePipelineCheckpointAction,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<PipelineCheckpointTerminalRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvePipelineCheckpointOutcome {
    pub checkpoint: PipelineResearchCheckpoint,
    pub context: crate::PipelineRunContext,
}

impl PipelineCheckpointRef {
    pub fn validate(&self) -> Result<()> {
        if self.checkpoint_id.is_nil() || self.digest.trim().is_empty() || self.digest.len() > 256 {
            Err(crate::Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl ResearchCheckpointDraft {
    pub fn validate(&self) -> Result<()> {
        self.inquiry.require_research()?;
        if valid_text(&self.question)
            && valid_text(&self.answer_criteria)
            && valid_text(&self.reason)
        {
            Ok(())
        } else {
            Err(crate::Error::InvalidArguments)
        }
    }
}

impl ResolvePipelineCheckpoint {
    pub fn validate(&self) -> Result<()> {
        self.checkpoint.validate()?;
        let terminal_valid = match self.action {
            ResolvePipelineCheckpointAction::Accept | ResolvePipelineCheckpointAction::Reject => {
                self.terminal.as_ref().is_some_and(|value| {
                    !value.result_id.is_nil()
                        && !value.output_id.is_nil()
                        && !value.output_digest.trim().is_empty()
                        && value.output_digest.len() <= 256
                })
            }
            ResolvePipelineCheckpointAction::Cancel => self.terminal.is_none(),
        };
        if self.request_id.is_nil()
            || self.producer_run_id.is_nil()
            || self.producer_run_revision < 1
            || !valid_text(&self.reason)
            || !terminal_valid
        {
            Err(crate::Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_CHECKPOINT_TEXT_BYTES
}
