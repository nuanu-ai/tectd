use super::AdvisoryOpportunityState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvisoryReason {
    WorkspaceDisabled,
    SessionSkip,
    RequestSkip,
    ChoiceSetNotApplicable,
    MatrixEvidenceUnresolved,
    MatrixSourceUnverified,
    MatrixTaskUnbound,
    MatrixSnapshotMissing,
    MatrixBindingMismatch,
    MatrixContextUnresolved,
    MatrixContextStale,
    MatrixAuthoritySchemaUnsupported,
    MatrixOperatingEvidenceUnresolved,
    DeterministicInputInvalid,
    CapabilityUnavailable,
    ProviderUnconfigured,
    BudgetPolicyInvalid,
    BudgetExhaustedAfterResponse,
    ConfigurationChanged,
    MatrixTaskRevisionChanged,
    MatrixVerificationStale,
    RecommendationPrepared,
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
            Self::ChoiceSetNotApplicable => "choice_set_not_applicable",
            Self::MatrixEvidenceUnresolved => "matrix_evidence_unresolved",
            Self::MatrixSourceUnverified => "matrix_source_unverified",
            Self::MatrixTaskUnbound => "matrix_task_unbound",
            Self::MatrixSnapshotMissing => "matrix_snapshot_missing",
            Self::MatrixBindingMismatch => "matrix_binding_mismatch",
            Self::MatrixContextUnresolved => "matrix_context_unresolved",
            Self::MatrixContextStale => "matrix_context_stale",
            Self::MatrixAuthoritySchemaUnsupported => "matrix_authority_schema_unsupported",
            Self::MatrixOperatingEvidenceUnresolved => "matrix_operating_evidence_unresolved",
            Self::DeterministicInputInvalid => "deterministic_input_invalid",
            Self::CapabilityUnavailable => "capability_unavailable",
            Self::ProviderUnconfigured => "provider_unconfigured",
            Self::BudgetPolicyInvalid => "budget_policy_invalid",
            Self::BudgetExhaustedAfterResponse => "budget_exhausted_after_response",
            Self::ConfigurationChanged => "configuration_changed",
            Self::MatrixTaskRevisionChanged => "matrix_task_revision_changed",
            Self::MatrixVerificationStale => "matrix_verification_stale",
            Self::RecommendationPrepared => "recommendation_prepared",
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
                | AdvisoryReason::ChoiceSetNotApplicable
                | AdvisoryReason::MatrixEvidenceUnresolved
                | AdvisoryReason::MatrixSourceUnverified
                | AdvisoryReason::MatrixTaskUnbound
                | AdvisoryReason::MatrixSnapshotMissing
                | AdvisoryReason::MatrixBindingMismatch
                | AdvisoryReason::MatrixContextUnresolved
                | AdvisoryReason::MatrixContextStale
                | AdvisoryReason::MatrixAuthoritySchemaUnsupported
                | AdvisoryReason::MatrixOperatingEvidenceUnresolved
                | AdvisoryReason::DeterministicInputInvalid
                | AdvisoryReason::CapabilityUnavailable
                | AdvisoryReason::ProviderUnconfigured
                | AdvisoryReason::BudgetPolicyInvalid
        ),
        AdvisoryOpportunityState::Prepared => matches!(
            reason,
            AdvisoryReason::DispatchAuthorized | AdvisoryReason::RecommendationPrepared
        ),
        AdvisoryOpportunityState::AwaitingResponse => matches!(
            reason,
            AdvisoryReason::DispatchAuthorized | AdvisoryReason::SendUnknown
        ),
        AdvisoryOpportunityState::Advised => matches!(reason, AdvisoryReason::ProviderResponse),
        AdvisoryOpportunityState::Invalidated => {
            matches!(
                reason,
                AdvisoryReason::ConfigurationChanged
                    | AdvisoryReason::MatrixTaskRevisionChanged
                    | AdvisoryReason::MatrixVerificationStale
            )
        }
        AdvisoryOpportunityState::Failed => matches!(
            reason,
            AdvisoryReason::ProviderFailure | AdvisoryReason::BudgetExhaustedAfterResponse
        ),
        AdvisoryOpportunityState::Unresolved => matches!(reason, AdvisoryReason::SendUnknown),
    }
}
