use super::{
    AdvisoryCapability, AdvisoryOpportunityInput, AdvisoryOpportunityState, AdvisoryReason,
    valid_sha256,
};
use uuid::Uuid;

pub(super) fn matrix_binding_invalid(input: &AdvisoryOpportunityInput) -> bool {
    match input.capability {
        AdvisoryCapability::EngineeringProfile => {
            input.target_kind != "matrix_task"
                || input.target_id.is_none()
                || input.matrix_task_revision != input.work_revision
                || input.matrix_task_revision.is_none()
                || input
                    .matrix_choice_set_digest
                    .as_ref()
                    .is_some_and(|digest| !valid_sha256(digest))
                || input
                    .matrix_verification_digest
                    .as_ref()
                    .is_some_and(|digest| !valid_sha256(digest))
                || (input.matrix_choice_set_digest.is_none()
                    && input.state != AdvisoryOpportunityState::NoCall)
                || (matches!(
                    input.state,
                    AdvisoryOpportunityState::Prepared
                        | AdvisoryOpportunityState::AwaitingResponse
                        | AdvisoryOpportunityState::Advised
                ) && input.matrix_verification_digest.is_none())
        }
        _ => {
            input.matrix_task_revision.is_some()
                || input.matrix_choice_set_digest.is_some()
                || input.matrix_verification_digest.is_some()
        }
    }
}

pub(super) fn matrix_reason_invalid(input: &AdvisoryOpportunityInput) -> bool {
    (matches!(
        input.primary_reason,
        AdvisoryReason::MatrixTaskRevisionChanged | AdvisoryReason::MatrixVerificationStale
    ) || matches!(
        input.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved
            | AdvisoryReason::MatrixSourceUnverified
            | AdvisoryReason::MatrixTaskUnbound
            | AdvisoryReason::MatrixSnapshotMissing
            | AdvisoryReason::MatrixBindingMismatch
            | AdvisoryReason::MatrixContextUnresolved
            | AdvisoryReason::MatrixContextStale
            | AdvisoryReason::MatrixAuthoritySchemaUnsupported
            | AdvisoryReason::MatrixOperatingEvidenceUnresolved
    )) && input.capability != AdvisoryCapability::EngineeringProfile
}

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
