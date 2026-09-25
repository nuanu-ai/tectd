use super::AdvisoryDispatch;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryBudgetReservation {
    pub dispatch_id: Uuid,
    pub policy_id: Uuid,
    pub policy_version: i64,
    pub policy_digest: String,
    pub policy_effective_from_unix_ms: i64,
    pub policy_effective_until_unix_ms: i64,
    pub request_sha256: String,
    pub request_utf8_bytes: i64,
    pub reserved_calls: i64,
    pub reserved_retry_dispatches: i64,
    pub remaining_elapsed_ms: i64,
    pub reserved_input_tokens: i64,
    pub reserved_output_tokens: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvisoryBudgetConsumption {
    pub dispatch_id: Uuid,
    pub policy_id: Uuid,
    pub policy_version: i64,
    pub policy_digest: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub monotonic_elapsed_ms: Option<i64>,
    pub unknown_usage: bool,
    pub exhausted_after_response: bool,
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
