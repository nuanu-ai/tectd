//! Jev advice: the workspace setting, the journal and the `advice` reply field.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use uuid::Uuid;

mod binding;
mod draft_view;
mod exchange;
mod intake;
mod intake_catalog;
mod pipeline;
mod pipeline_catalog;
mod point;
mod questions;
mod shaping;
mod sizing;
mod sizing_catalog;
mod sizing_state;
#[cfg(test)]
mod test_drafts;
mod written;
pub use binding::{AdviceDeparture, BindingPoint, CandidateAdvice, validate_advice_departures};
pub use exchange::{AdviceCompletion, AdviceExchange, AdviceFailure, AdviceReply};
pub use intake::{INTAKE_NOTE, INTAKE_POINT, IntakeAdvice, IntakePlanning};
pub use pipeline::{PIPELINE_NOTE, PIPELINE_POINT, PipelineAdvice, PipelineDraft};
pub use questions::{ADVICE_ABSTAIN, AdviceAnswer, AdviceQuestion, AdviceQuestionSet};
pub use shaping::AdviceShaping;
pub use sizing::{SIZING_NOTE, SIZING_POINT, SizingAdvice, SizingLevel};
pub use sizing_state::{SizingDraft, SizingScope, SizingWord, SizingWordSource, SizingWordsBasis};

pub const MAX_ADVICE_POINT_BYTES: usize = 64;
pub const MAX_ADVICE_ROUTE_BYTES: usize = 128;
pub const MAX_ADVICE_PHASE_BYTES: usize = 256;
pub const MAX_ADVICE_PAGE: u32 = 100;

/// One row per workspace. No row means Jev is disabled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdviceConfig {
    pub enabled: bool,
    /// Calls to Jev per UTC day. Present whenever Jev is enabled.
    pub daily_call_limit: Option<i64>,
    pub revision: i64,
    pub updated_at: String,
    pub updated_by_session: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdviceConfigStatus {
    pub config: Option<AdviceConfig>,
    pub calls_today: i64,
    pub provider_configured: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigureAdvice {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daily_call_limit: Option<i64>,
    /// Revision the caller read; 0 when the workspace has no setting yet.
    pub expected_revision: i64,
}

impl ConfigureAdvice {
    pub fn validate(&self) -> Result<()> {
        if self.expected_revision < 0
            || self.daily_call_limit.is_some_and(|limit| limit <= 0)
            || self.enabled && self.daily_call_limit.is_none()
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceOutcome {
    Advised,
    Abstained,
    Unavailable,
}

impl AdviceOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Advised => "advised",
            Self::Abstained => "abstained",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Closed list of why an enabled Jev gave no advice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceUnavailableReason {
    ProviderUnconfigured,
    DailyLimitReached,
    Timeout,
    TransportError,
    HttpStatus,
    InvalidResponse,
    RequestTooLarge,
    /// The point's largest possible field would not fit the command reply.
    AdviceTooLarge,
    /// The Program has no product level to judge against; Jev is not asked.
    ModeNotSet,
    /// The Program's text is erased or not readable, so there is no statement to
    /// send; Jev is not asked.
    ProgramUnavailable,
}

impl AdviceUnavailableReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderUnconfigured => "provider_unconfigured",
            Self::DailyLimitReached => "daily_limit_reached",
            Self::Timeout => "timeout",
            Self::TransportError => "transport_error",
            Self::HttpStatus => "http_status",
            Self::InvalidResponse => "invalid_response",
            Self::RequestTooLarge => "request_too_large",
            Self::AdviceTooLarge => "advice_too_large",
            Self::ModeNotSet => "mode_not_set",
            Self::ProgramUnavailable => "program_unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdviceSendCertainty {
    NotSent,
    Sent,
    /// The request left; no answer is recorded.
    SentUnknown,
}

impl AdviceSendCertainty {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotSent => "not_sent",
            Self::Sent => "sent",
            Self::SentUnknown => "sent_unknown",
        }
    }
}

/// The `advice` field of a command reply. A disabled Jev has no field at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AdviceField {
    Advised {
        #[serde(flatten)]
        content: Map<String, Value>,
    },
    Abstained,
    Unavailable {
        reason: AdviceUnavailableReason,
        message: String,
    },
}

impl AdviceField {
    pub fn unavailable(reason: AdviceUnavailableReason, detail: &str) -> Self {
        Self::Unavailable {
            reason,
            message: format!("Jev advice is not available: {detail}"),
        }
    }

    /// The field of a point that is not asked because its largest advice on
    /// `subject` would not fit the command reply.
    pub fn too_large(subject: &str) -> Self {
        Self::unavailable(
            AdviceUnavailableReason::AdviceTooLarge,
            &format!("advice for {subject} would not fit the command reply"),
        )
    }

    pub const fn outcome(&self) -> AdviceOutcome {
        match self {
            Self::Advised { .. } => AdviceOutcome::Advised,
            Self::Abstained => AdviceOutcome::Abstained,
            Self::Unavailable { .. } => AdviceOutcome::Unavailable,
        }
    }

    pub const fn reason(&self) -> Option<AdviceUnavailableReason> {
        match self {
            Self::Unavailable { reason, .. } => Some(*reason),
            _ => None,
        }
    }
}

/// What the work was when an advice point fired. Absent parts stay empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdviceWorkRefs {
    /// Idempotency request id of the agent's command in which the point fired.
    pub request_id: Option<Uuid>,
    pub program_id: Option<Uuid>,
    pub candidate_set_id: Option<Uuid>,
    pub scope_id: Option<Uuid>,
    pub slice_id: Option<Uuid>,
    pub phase: Option<String>,
}

impl AdviceWorkRefs {
    pub fn validate(&self) -> Result<()> {
        let ids = [
            self.request_id,
            self.program_id,
            self.candidate_set_id,
            self.scope_id,
            self.slice_id,
        ];
        if ids.iter().flatten().any(Uuid::is_nil)
            || self.phase.as_deref().is_some_and(|phase| {
                phase.trim().is_empty() || phase.len() > MAX_ADVICE_PHASE_BYTES
            })
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

/// A journal row to insert: a call about to be sent, or a point that fired without a call.
#[derive(Debug, Clone, PartialEq)]
pub struct NewAdviceEntry {
    pub point: String,
    pub route: String,
    pub work: AdviceWorkRefs,
    /// `None` reserves a call (`sent_unknown`, empty outcome) to be completed after
    /// the answer. `Some` records the reply field of a point that made no call.
    pub not_called: Option<AdviceField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceJournalEntry {
    pub id: Uuid,
    pub created_at: String,
    pub workspace_id: Uuid,
    pub session_id: Uuid,
    pub point: String,
    pub route: String,
    #[serde(flatten)]
    pub work: AdviceWorkRefs,
    pub called: bool,
    pub outcome: Option<AdviceOutcome>,
    pub reason: Option<AdviceUnavailableReason>,
    pub send_certainty: AdviceSendCertainty,
    pub model: Option<String>,
    pub request_bytes: Option<i64>,
    pub response_bytes: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    /// Exactly the reply field the agent received for this row.
    pub advice: Option<AdviceField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdviceAuditQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<AdviceOutcome>,
    /// Inclusive RFC 3339 lower bound of the row time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    /// Exclusive RFC 3339 upper bound of the row time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Uuid>,
    /// Rows per page; 0 returns the counters alone.
    pub limit: u32,
    /// Whether rows carry the request and answer bodies.
    pub bodies: bool,
}

impl AdviceAuditQuery {
    pub fn validate(&self) -> Result<()> {
        if let Some(point) = &self.point {
            validate_advice_point(point)?;
        }
        let since = self
            .since
            .as_deref()
            .map(crate::knowledge_time::parse_rfc3339);
        let until = self
            .until
            .as_deref()
            .map(crate::knowledge_time::parse_rfc3339);
        if self.limit > MAX_ADVICE_PAGE
            || self.after.is_some_and(|id| id.is_nil())
            || since == Some(None)
            || until == Some(None)
            || matches!((since, until), (Some(since), Some(until)) if since > until)
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

/// Counts over the point and time filters, independent of outcome filter and paging.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdviceCounters {
    pub called: i64,
    pub not_called: i64,
    pub advised: i64,
    pub abstained: i64,
    pub unavailable: i64,
    /// Sent calls whose answer is not recorded.
    pub pending: i64,
    pub reasons: BTreeMap<AdviceUnavailableReason, i64>,
}

impl AdviceCounters {
    pub fn add(
        &mut self,
        called: bool,
        outcome: Option<AdviceOutcome>,
        reason: Option<AdviceUnavailableReason>,
        count: i64,
    ) {
        *if called {
            &mut self.called
        } else {
            &mut self.not_called
        } += count;
        *match outcome {
            Some(AdviceOutcome::Advised) => &mut self.advised,
            Some(AdviceOutcome::Abstained) => &mut self.abstained,
            Some(AdviceOutcome::Unavailable) => &mut self.unavailable,
            None => &mut self.pending,
        } += count;
        if let Some(reason) = reason {
            *self.reasons.entry(reason).or_default() += count;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceAuditPage {
    pub counters: AdviceCounters,
    pub entries: Vec<AdviceJournalEntry>,
    pub next_after: Option<Uuid>,
}

/// Advice point code, such as `sizing`.
pub fn validate_advice_point(value: &str) -> Result<()> {
    label(value, MAX_ADVICE_POINT_BYTES, b"_")
}

/// Route name of the command in which a point fired, such as `scope.candidates.save`.
pub fn validate_advice_route(value: &str) -> Result<()> {
    label(value, MAX_ADVICE_ROUTE_BYTES, b"_.")
}

fn label(value: &str, maximum: usize, punctuation: &[u8]) -> Result<()> {
    if value.is_empty()
        || value.len() > maximum
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || punctuation.contains(&byte)
        })
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
