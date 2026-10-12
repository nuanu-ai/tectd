//! The `advice` field on a command reply. One encoder and one budget serve both the
//! question "would the largest field fit?" asked before Jev is called and the reply
//! that finally leaves.
use crate::responses;
use serde_json::Value;
use tect_application::AdviceOutputGuard;
use tect_domain::{AdviceField, Error, Result};

/// A command reply built as `main` builds it, and the capacity its caller gave.
pub(crate) struct ReplyRoom<'a> {
    pub reply: &'a Value,
    pub capacity: usize,
}

impl AdviceOutputGuard for ReplyRoom<'_> {
    fn check_reply(&self, advice: &AdviceField) -> Result<()> {
        attach(self.reply.clone(), advice, self.capacity).map(|_| ())
    }
}

/// The reply a command returns: untouched when there is no field, so that a disabled
/// Jev answers byte for byte as `main` does.
pub(crate) fn finish(reply: Value, advice: Option<AdviceField>, capacity: usize) -> Result<Value> {
    match advice {
        Some(advice) => attach(reply, &advice, capacity),
        None => Ok(reply),
    }
}

/// The reply with the field attached, held to the read budget every save reply obeys.
fn attach(mut reply: Value, advice: &AdviceField, capacity: usize) -> Result<Value> {
    let fields = reply.as_object_mut().ok_or(Error::InternalInvariant)?;
    let advice = serde_json::to_value(advice).map_err(|_| Error::TransportUnavailable)?;
    fields.insert("advice".to_owned(), advice);
    if responses::encoded_len(&reply)? > capacity.min(crate::json_fragment::READ_BUDGET) {
        return Err(Error::RequestTooLarge);
    }
    Ok(reply)
}

#[cfg(test)]
pub(crate) mod tests;
