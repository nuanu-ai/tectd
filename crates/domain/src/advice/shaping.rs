//! What an advice point adds to the generic call: the content of its reply field, and
//! what is kept of the answers for the review of the draft.
use super::{AdviceAnswer, AdviceField, CandidateAdvice};
use std::collections::BTreeMap;

/// The pure step between Jev's accepted answers and the `advice` field of a reply.
pub trait AdviceShaping: Send + Sync {
    /// The field for accepted answers of which at least one is not `ABSTAIN`.
    fn shape(&self, answers: &BTreeMap<String, AdviceAnswer>) -> AdviceField;

    /// A field no smaller than anything `shape` returns for this request. The command
    /// reply is measured with it before Jev is asked.
    fn largest(&self) -> AdviceField;

    /// What the advice is about, as it reads in a message: `this draft (2 candidates,
    /// 7 included items)`.
    fn subject(&self) -> String;

    /// What the answers say about each candidate asked about, as it is kept for the
    /// review of the draft. It is read for answers `shape` made an advised field of.
    /// A point no review is held to keeps nothing.
    fn kept(&self, answers: &BTreeMap<String, AdviceAnswer>) -> Vec<CandidateAdvice>;
}
