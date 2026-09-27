use super::*;

#[cfg(test)]
mod recovery_tests {
    use super::*;

    #[test]
    fn only_unsent_authorized_attempt_can_enter_transport_window() {
        assert_eq!(
            matrix_recovery_window(
                AdvisoryOpportunityState::Prepared,
                AdvisoryDispatchState::Authorized,
                AdvisorySendCertainty::NotSent,
                None,
            ),
            MatrixRecoveryWindow::Authorized,
        );
        for (state, certainty, outcome) in [
            (
                AdvisoryDispatchState::Sending,
                AdvisorySendCertainty::SentUnknown,
                None,
            ),
            (
                AdvisoryDispatchState::Cancelled,
                AdvisorySendCertainty::NotSent,
                None,
            ),
            (
                AdvisoryDispatchState::Sealed,
                AdvisorySendCertainty::SentUnknown,
                Some(AdvisoryDispatchOutcome::ProviderFailure),
            ),
            (
                AdvisoryDispatchState::Sealed,
                AdvisorySendCertainty::Sent,
                Some(AdvisoryDispatchOutcome::ProviderResponse),
            ),
        ] {
            assert_eq!(
                matrix_recovery_window(
                    AdvisoryOpportunityState::Prepared,
                    state,
                    certainty,
                    outcome
                ),
                MatrixRecoveryWindow::ReceiptOnly,
            );
        }
    }

    #[test]
    fn only_sealed_saved_response_can_enter_parse_window() {
        assert_eq!(
            matrix_recovery_window(
                AdvisoryOpportunityState::AwaitingResponse,
                AdvisoryDispatchState::Sealed,
                AdvisorySendCertainty::Sent,
                Some(AdvisoryDispatchOutcome::ProviderResponse),
            ),
            MatrixRecoveryWindow::SealedResponse,
        );
        for (state, certainty, outcome) in [
            (
                AdvisoryDispatchState::Sending,
                AdvisorySendCertainty::SentUnknown,
                None,
            ),
            (
                AdvisoryDispatchState::Sealed,
                AdvisorySendCertainty::SentUnknown,
                Some(AdvisoryDispatchOutcome::ProviderFailure),
            ),
            (
                AdvisoryDispatchState::Cancelled,
                AdvisorySendCertainty::NotSent,
                None,
            ),
        ] {
            assert_eq!(
                matrix_recovery_window(
                    AdvisoryOpportunityState::AwaitingResponse,
                    state,
                    certainty,
                    outcome
                ),
                MatrixRecoveryWindow::ReceiptOnly,
            );
        }
    }

    #[test]
    fn legacy_verification_only_reconciles_sealed_response() {
        assert!(!legacy_reconciliation_allowed(
            MatrixRecoveryWindow::Authorized
        ));
        assert!(legacy_reconciliation_allowed(
            MatrixRecoveryWindow::SealedResponse
        ));
        assert!(!legacy_reconciliation_allowed(
            MatrixRecoveryWindow::ReceiptOnly
        ));
    }
}
