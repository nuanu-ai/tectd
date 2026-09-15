use crate::{Error, PlanningTaskContext, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineInquiryTopicLevel {
    Program,
    Scope,
    Slice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineDecisionOutcome {
    Decision,
    Recommendation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineInquiryCompletion {
    Research {
        allow_inconclusive: bool,
    },
    Decision {
        requested_outcome: PipelineDecisionOutcome,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineInquiryContract {
    pub topic_level: PipelineInquiryTopicLevel,
    pub task_context: PlanningTaskContext,
    pub completion: PipelineInquiryCompletion,
}

impl PipelineInquiryContract {
    pub fn validate(&self) -> Result<()> {
        self.task_context.validate()
    }

    pub fn is_research(&self) -> bool {
        matches!(self.completion, PipelineInquiryCompletion::Research { .. })
    }

    pub fn is_decision(&self) -> bool {
        matches!(self.completion, PipelineInquiryCompletion::Decision { .. })
    }

    pub fn require_research(&self) -> Result<()> {
        self.validate()?;
        if self.is_research() {
            Ok(())
        } else {
            Err(Error::InvalidArguments)
        }
    }
}
