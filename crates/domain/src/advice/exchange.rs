//! What one provider call produced and how it completes its journal row.
use super::{AdviceAnswer, AdviceField, AdviceSendCertainty, AdviceUnavailableReason, questions};
use std::collections::BTreeMap;

/// Facts of one call. Whatever is not known stays `None` and is never replaced by zero.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdviceExchange {
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    /// Bytes as sent.
    pub request_body: Option<Vec<u8>>,
    /// Bytes as received.
    pub response_body: Option<Vec<u8>>,
}

/// Accepted answers to every question asked.
#[derive(Debug, Clone, PartialEq)]
pub struct AdviceReply {
    pub answers: BTreeMap<String, AdviceAnswer>,
    pub exchange: AdviceExchange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdviceFailure {
    pub reason: AdviceUnavailableReason,
    pub send_certainty: AdviceSendCertainty,
    /// What happened, in English, for the reply message.
    pub detail: String,
    pub exchange: AdviceExchange,
}

/// The values that complete a reserved journal row.
#[derive(Debug, Clone, PartialEq)]
pub struct AdviceCompletion {
    pub send_certainty: AdviceSendCertainty,
    pub advice: AdviceField,
    pub exchange: AdviceExchange,
}

impl AdviceCompletion {
    /// A provider failure is never an error of the command: it becomes `unavailable`.
    pub fn from_provider(result: std::result::Result<AdviceReply, AdviceFailure>) -> Self {
        match result {
            Ok(reply) => Self {
                send_certainty: AdviceSendCertainty::Sent,
                advice: if questions::abstained(&reply.answers) {
                    AdviceField::Abstained
                } else {
                    match serde_json::to_value(&reply.answers) {
                        Ok(answers) => AdviceField::Advised {
                            content: [("answers".to_owned(), answers)].into_iter().collect(),
                        },
                        Err(_) => AdviceField::unavailable(
                            AdviceUnavailableReason::InvalidResponse,
                            "the answers could not be represented",
                        ),
                    }
                },
                exchange: reply.exchange,
            },
            Err(failure) => Self {
                send_certainty: failure.send_certainty,
                advice: AdviceField::unavailable(failure.reason, &failure.detail),
                exchange: failure.exchange,
            },
        }
    }
}

impl AdviceFailure {
    /// A point that fired without any call to the provider.
    pub fn not_called(reason: AdviceUnavailableReason, detail: impl Into<String>) -> Self {
        Self {
            reason,
            send_certainty: AdviceSendCertainty::NotSent,
            detail: detail.into(),
            exchange: AdviceExchange::default(),
        }
    }
}
