//! Jev's advice binds: the agent follows it and departs only where it holds the advice
//! plainly wrong, stating why. Here is what is kept of Jev's answer about one
//! candidate, so that the review of the draft can be held to it.
use super::{PIPELINE_POINT, SIZING_POINT, SizingLevel};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The advice points a ready review is held to. `intake` is not one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingPoint {
    Sizing,
    Pipeline,
}

impl BindingPoint {
    /// The code of the advice point, as the journal holds it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sizing => SIZING_POINT,
            Self::Pipeline => PIPELINE_POINT,
        }
    }
}

/// Jev's answer about one candidate a draft save created or changed, as it is kept
/// for the review of that draft.
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateAdvice {
    pub level: SizingLevel,
    pub candidate_id: Uuid,
    /// The revision the store holds the candidate under: the answer is about that one.
    pub candidate_revision: i64,
    /// Whether the answer asks for the candidate to be changed.
    pub asks_change: bool,
    /// The candidate's entry of the reply field: what Jev chose and its
    /// probabilities, and no text of the draft.
    pub answer: Value,
}
