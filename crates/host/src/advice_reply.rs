//! The `advice` field on a command reply, and the field of a second point beside it:
//! `pipeline_advice` on a Slice draft save, `model_advice` on a recorded input. One
//! encoder and one budget serve both the question "would the largest field fit?"
//! asked before Jev is called and the reply that finally leaves.
use crate::responses;
use serde_json::Value;
use tect_application::AdviceOutputGuard;
use tect_domain::{AdviceField, Error, Result};

/// The reply key of the field the advice point of a command gives.
const ADVICE: &str = "advice";
/// The reply key of the `pipeline` point, the second point on a Slice draft save.
const PIPELINE_ADVICE: &str = "pipeline_advice";
/// The reply key of the `model` point, the second point on a recorded input.
const MODEL_ADVICE: &str = "model_advice";

/// A command reply built as `main` builds it, and the capacity its caller gave.
pub(crate) struct ReplyRoom<'a> {
    pub reply: &'a Value,
    pub capacity: usize,
}

impl AdviceOutputGuard for ReplyRoom<'_> {
    fn check_reply(&self, advice: &AdviceField) -> Result<()> {
        attach(self.reply.clone(), ADVICE, advice, self.capacity).map(|_| ())
    }
}

/// A Slice draft save reply that already holds whatever `advice` field `sizing` gave,
/// and the capacity its caller gave: the room the `pipeline_advice` field has.
pub(crate) struct PipelineReplyRoom<'a> {
    pub reply: &'a Value,
    pub capacity: usize,
}

impl AdviceOutputGuard for PipelineReplyRoom<'_> {
    fn check_reply(&self, advice: &AdviceField) -> Result<()> {
        attach(self.reply.clone(), PIPELINE_ADVICE, advice, self.capacity).map(|_| ())
    }
}

/// A recorded-input reply as it stands when `model` is asked, and the capacity its
/// caller gave: the room the `model_advice` field has. For an input of a candidate
/// set the reply already holds whatever `advice` field `intake` gave; for an input of
/// a Program it is the reply as the command built it.
pub(crate) struct ModelReplyRoom<'a> {
    pub reply: &'a Value,
    pub capacity: usize,
}

impl AdviceOutputGuard for ModelReplyRoom<'_> {
    fn check_reply(&self, advice: &AdviceField) -> Result<()> {
        attach(self.reply.clone(), MODEL_ADVICE, advice, self.capacity).map(|_| ())
    }
}

/// The reply a command returns: untouched when there is no field, so that a disabled
/// Jev answers byte for byte as `main` does.
pub(crate) fn finish(reply: Value, advice: Option<AdviceField>, capacity: usize) -> Result<Value> {
    match advice {
        Some(advice) => attach(reply, ADVICE, &advice, capacity),
        None => Ok(reply),
    }
}

/// The same for the `pipeline_advice` field: untouched when the point gave none.
pub(crate) fn finish_pipeline(
    reply: Value,
    advice: Option<AdviceField>,
    capacity: usize,
) -> Result<Value> {
    match advice {
        Some(advice) => attach(reply, PIPELINE_ADVICE, &advice, capacity),
        None => Ok(reply),
    }
}

/// A copy of a Slice draft save reply that holds the field the `pipeline` point gives
/// when it has no room. The first point of the save is measured on it. Nothing is
/// checked here: a disabled Jev must not meet a new failure.
pub(crate) fn with_pipeline_reserve(reply: &Value, without_room: &AdviceField) -> Value {
    let mut measured = reply.clone();
    if let (Some(fields), Ok(field)) =
        (measured.as_object_mut(), serde_json::to_value(without_room))
    {
        fields.insert(PIPELINE_ADVICE.to_owned(), field);
    }
    measured
}

/// The reply a recorded input returns once `model` is decided: with the
/// `model_advice` field attached, or untouched when the point gave none.
pub(crate) fn finish_model(
    reply: Value,
    advice: Option<AdviceField>,
    capacity: usize,
) -> Result<Value> {
    match advice {
        Some(advice) => attach(reply, MODEL_ADVICE, &advice, capacity),
        None => Ok(reply),
    }
}

/// A copy of a recorded-input reply that holds the field the `model` point gives when
/// it has no room. The first point of the command is measured on it. Nothing is
/// checked here: a disabled Jev must not meet a new failure.
pub(crate) fn with_model_reserve(reply: &Value, without_room: &AdviceField) -> Value {
    let mut measured = reply.clone();
    if let (Some(fields), Ok(field)) =
        (measured.as_object_mut(), serde_json::to_value(without_room))
    {
        fields.insert(MODEL_ADVICE.to_owned(), field);
    }
    measured
}

/// The reply with the field attached under its key, held to the read budget every
/// save reply obeys.
fn attach(mut reply: Value, key: &str, advice: &AdviceField, capacity: usize) -> Result<Value> {
    let fields = reply.as_object_mut().ok_or(Error::InternalInvariant)?;
    let advice = serde_json::to_value(advice).map_err(|_| Error::TransportUnavailable)?;
    fields.insert(key.to_owned(), advice);
    if responses::encoded_len(&reply)? > capacity.min(crate::json_fragment::READ_BUDGET) {
        return Err(Error::RequestTooLarge);
    }
    Ok(reply)
}

#[cfg(test)]
pub(crate) mod model_tests;

#[cfg(test)]
pub(crate) mod tests;
