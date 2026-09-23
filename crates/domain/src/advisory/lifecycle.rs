use super::config::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryRequestPreference, MAX_ADVISORY_KEY_BYTES,
    MAX_ADVISORY_TEXT_BYTES, validate_non_secret_identifier,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryOpportunityState {
    Prepared,
    NoCall,
    AwaitingResponse,
    Advised,
    Invalidated,
    Failed,
    Unresolved,
}

impl AdvisoryOpportunityState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::NoCall => "no_call",
            Self::AwaitingResponse => "awaiting_response",
            Self::Advised => "advised",
            Self::Invalidated => "invalidated",
            Self::Failed => "failed",
            Self::Unresolved => "unresolved",
        }
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Prepared,
                Self::NoCall | Self::AwaitingResponse | Self::Invalidated | Self::Failed
            ) | (
                Self::AwaitingResponse,
                Self::Advised | Self::Invalidated | Self::Failed | Self::Unresolved
            ) | (Self::Advised, Self::Invalidated)
                | (
                    Self::Unresolved,
                    Self::Advised | Self::Invalidated | Self::Failed
                )
                | (Self::Failed, Self::AwaitingResponse)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryReason {
    WorkspaceDisabled,
    SessionSkip,
    RequestSkip,
    DeterministicInputInvalid,
    CapabilityUnavailable,
    ProviderUnconfigured,
    BudgetPolicyInvalid,
    ConfigurationChanged,
    DispatchAuthorized,
    ProviderResponse,
    ProviderFailure,
    SendUnknown,
}

impl AdvisoryReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkspaceDisabled => "workspace_disabled",
            Self::SessionSkip => "session_skip",
            Self::RequestSkip => "request_skip",
            Self::DeterministicInputInvalid => "deterministic_input_invalid",
            Self::CapabilityUnavailable => "capability_unavailable",
            Self::ProviderUnconfigured => "provider_unconfigured",
            Self::BudgetPolicyInvalid => "budget_policy_invalid",
            Self::ConfigurationChanged => "configuration_changed",
            Self::DispatchAuthorized => "dispatch_authorized",
            Self::ProviderResponse => "provider_response",
            Self::ProviderFailure => "provider_failure",
            Self::SendUnknown => "send_unknown",
        }
    }
}

pub const fn advisory_reason_matches_state(
    state: AdvisoryOpportunityState,
    reason: AdvisoryReason,
) -> bool {
    match state {
        AdvisoryOpportunityState::NoCall => matches!(
            reason,
            AdvisoryReason::WorkspaceDisabled
                | AdvisoryReason::SessionSkip
                | AdvisoryReason::RequestSkip
                | AdvisoryReason::DeterministicInputInvalid
                | AdvisoryReason::CapabilityUnavailable
                | AdvisoryReason::ProviderUnconfigured
                | AdvisoryReason::BudgetPolicyInvalid
        ),
        AdvisoryOpportunityState::Prepared => matches!(reason, AdvisoryReason::DispatchAuthorized),
        AdvisoryOpportunityState::AwaitingResponse => matches!(
            reason,
            AdvisoryReason::DispatchAuthorized | AdvisoryReason::SendUnknown
        ),
        AdvisoryOpportunityState::Advised => matches!(reason, AdvisoryReason::ProviderResponse),
        AdvisoryOpportunityState::Invalidated => {
            matches!(reason, AdvisoryReason::ConfigurationChanged)
        }
        AdvisoryOpportunityState::Failed => matches!(reason, AdvisoryReason::ProviderFailure),
        AdvisoryOpportunityState::Unresolved => matches!(reason, AdvisoryReason::SendUnknown),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisorySendCertainty {
    NotSent,
    Sent,
    SentUnknown,
}

impl AdvisorySendCertainty {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotSent => "not_sent",
            Self::Sent => "sent",
            Self::SentUnknown => "sent_unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryDispatchState {
    Authorized,
    Sending,
    Sealed,
    Cancelled,
}

impl AdvisoryDispatchState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Authorized => "authorized",
            Self::Sending => "sending",
            Self::Sealed => "sealed",
            Self::Cancelled => "cancelled",
        }
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Authorized, Self::Sending | Self::Cancelled) | (Self::Sending, Self::Sealed)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryRetryBasis {
    Initial,
    ProvenNotSent,
    KnownRetryableResponse,
    VerifiedProviderIdempotency,
}

impl AdvisoryRetryBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::ProvenNotSent => "proven_not_sent",
            Self::KnownRetryableResponse => "known_retryable_response",
            Self::VerifiedProviderIdempotency => "verified_provider_idempotency",
        }
    }
}

pub const fn advisory_retry_permitted(
    previous_send: AdvisorySendCertainty,
    basis: AdvisoryRetryBasis,
) -> bool {
    matches!(
        (previous_send, basis),
        (
            AdvisorySendCertainty::NotSent,
            AdvisoryRetryBasis::ProvenNotSent
        )
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryDispatchOutcome {
    ProviderResponse,
    ProviderFailure,
}

impl AdvisoryDispatchOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ProviderResponse => "provider_response",
            Self::ProviderFailure => "provider_failure",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryOpportunityInput {
    pub session_id: Uuid,
    pub authorized_actor_id: Uuid,
    pub capability: AdvisoryCapability,
    pub decision_point: AdvisoryDecisionPoint,
    pub decision_point_version: i32,
    pub workflow_occurrence_key: String,
    pub target_kind: String,
    pub target_id: Option<Uuid>,
    pub work_revision: Option<i64>,
    pub source_ref: Option<String>,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
    pub config_revision: i64,
    pub material_digest: String,
    pub state: AdvisoryOpportunityState,
    pub primary_reason: AdvisoryReason,
}

impl AdvisoryOpportunityInput {
    pub fn validate(&self) -> Result<()> {
        if self.session_id.is_nil()
            || self.authorized_actor_id.is_nil()
            || self.decision_point_version < 1
            || self.workflow_occurrence_key.is_empty()
            || self.workflow_occurrence_key.len() > MAX_ADVISORY_KEY_BYTES
            || self.target_kind.is_empty()
            || self.target_kind.len() > MAX_ADVISORY_TEXT_BYTES
            || self.source_ref.as_ref().is_some_and(|value| {
                value.is_empty() || value.len() > MAX_ADVISORY_TEXT_BYTES || value.contains('\0')
            })
            || self.config_revision < 0
            || !valid_sha256(&self.material_digest)
            || self.work_revision.is_some_and(|revision| revision < 1)
            || !self.decision_point.supports(self.capability)
        {
            return Err(Error::InvalidArguments);
        }
        if !advisory_reason_matches_state(self.state, self.primary_reason)
            || matches!(self.primary_reason, AdvisoryReason::SessionSkip)
                && self.session_preference != AdvisoryRequestPreference::Skip
            || matches!(self.primary_reason, AdvisoryReason::RequestSkip)
                && self.request_preference != AdvisoryRequestPreference::Skip
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryOpportunity {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub session_id: Uuid,
    pub authorized_actor_id: Uuid,
    pub capability: AdvisoryCapability,
    pub decision_point: AdvisoryDecisionPoint,
    pub decision_point_version: i32,
    pub workflow_occurrence_key: String,
    pub target_kind: String,
    pub target_id: Option<Uuid>,
    pub work_revision: Option<i64>,
    pub source_ref: Option<String>,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
    pub config_revision: i64,
    pub material_digest: String,
    pub state: AdvisoryOpportunityState,
    pub primary_reason: AdvisoryReason,
    pub provider_called: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryDispatchAuthorization {
    pub dispatch_id: Uuid,
    pub opportunity_id: Uuid,
    pub predecessor_dispatch_id: Option<Uuid>,
    pub attempt_number: i32,
    pub retry_basis: AdvisoryRetryBasis,
    pub provider: String,
    pub model: String,
    pub configuration_snapshot: serde_json::Value,
    pub configuration_digest: String,
    pub material_digest: String,
    pub payload_digest: String,
    pub request_payload: Vec<u8>,
}

impl AdvisoryDispatchAuthorization {
    pub fn validate(&self) -> Result<()> {
        if self.dispatch_id.is_nil()
            || self.opportunity_id.is_nil()
            || self.predecessor_dispatch_id.is_some_and(|id| id.is_nil())
            || self.provider.is_empty()
            || self.provider.len() > MAX_ADVISORY_TEXT_BYTES
            || self.model.is_empty()
            || self.model.len() > MAX_ADVISORY_TEXT_BYTES
            || !self.configuration_snapshot.is_object()
            || !valid_sha256(&self.configuration_digest)
            || !valid_sha256(&self.material_digest)
            || !valid_sha256(&self.payload_digest)
            || self.request_payload.is_empty()
            || self.attempt_number < 1
        {
            return Err(Error::InvalidArguments);
        }
        if self.attempt_number == 1
            && (self.predecessor_dispatch_id.is_some()
                || self.retry_basis != AdvisoryRetryBasis::Initial)
            || self.attempt_number > 1
                && (self.predecessor_dispatch_id.is_none()
                    || self.retry_basis == AdvisoryRetryBasis::Initial)
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryDispatchSeal {
    pub dispatch_id: Uuid,
    pub send_certainty: AdvisorySendCertainty,
    pub outcome: AdvisoryDispatchOutcome,
    pub response_payload: Option<Vec<u8>>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub raw_response_ref: Option<String>,
}

impl AdvisoryDispatchSeal {
    pub fn validate(&self) -> Result<()> {
        if self.dispatch_id.is_nil()
            || self.input_tokens.is_some_and(|value| value < 0)
            || self.output_tokens.is_some_and(|value| value < 0)
            || self.latency_ms.is_some_and(|value| value < 0)
            || self.raw_response_ref.as_ref().is_some_and(|value| {
                value.is_empty() || value.len() > MAX_ADVISORY_TEXT_BYTES || value.contains('\0')
            })
            || self.outcome == AdvisoryDispatchOutcome::ProviderResponse
                && (self.send_certainty != AdvisorySendCertainty::Sent
                    || self.response_payload.is_none())
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvisoryDispatch {
    pub id: Uuid,
    pub opportunity_id: Uuid,
    pub predecessor_dispatch_id: Option<Uuid>,
    pub attempt_number: i32,
    pub provider: String,
    pub model: String,
    pub configuration_digest: String,
    pub material_digest: String,
    pub payload_digest: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub latency_ms: Option<i64>,
    pub state: AdvisoryDispatchState,
    pub send_certainty: AdvisorySendCertainty,
    pub outcome: Option<AdvisoryDispatchOutcome>,
    pub retry_basis: AdvisoryRetryBasis,
    pub raw_response_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryDispatchStart {
    pub dispatch: AdvisoryDispatch,
    pub should_send: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdvisoryCancellationOutcome {
    Cancelled,
    DeliveryMayHaveOccurred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryDispatchCancellation {
    pub dispatch: AdvisoryDispatch,
    pub outcome: AdvisoryCancellationOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdvisoryReconciliationEvidence {
    Inconclusive {
        dispatch_id: Uuid,
    },
    ConfirmedSent(AdvisoryDispatchSeal),
    ConfirmedNotSent {
        dispatch_id: Uuid,
        evidence_ref: String,
    },
}

impl AdvisoryReconciliationEvidence {
    pub fn dispatch_id(&self) -> Uuid {
        match self {
            Self::Inconclusive { dispatch_id } | Self::ConfirmedNotSent { dispatch_id, .. } => {
                *dispatch_id
            }
            Self::ConfirmedSent(seal) => seal.dispatch_id,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.dispatch_id().is_nil() {
            return Err(Error::InvalidArguments);
        }
        match self {
            Self::Inconclusive { .. } => Ok(()),
            Self::ConfirmedSent(seal) => {
                seal.validate()?;
                if seal.send_certainty != AdvisorySendCertainty::Sent {
                    return Err(Error::InvalidArguments);
                }
                Ok(())
            }
            Self::ConfirmedNotSent { evidence_ref, .. } => {
                validate_non_secret_identifier(evidence_ref)
            }
        }
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
