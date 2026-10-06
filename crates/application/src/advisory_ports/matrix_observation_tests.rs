use super::*;

#[test]
fn common_raw_conversion_preserves_context_or_historical_absence() {
    for context in [
        None,
        Some(crate::AdvisoryProviderTransportContext {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: AdvisoryDispatchOutcome::ProviderFailure,
            raw_response_ref: Some("original-ref".into()),
            provider_failure_code: Some("http-status".into()),
        }),
    ] {
        let original = MatrixProviderObservation {
            response_payload: Some(b"opaque".to_vec()),
            http_status: Some(500),
            input_tokens: None,
            output_tokens: None,
            legacy_response: None,
            response_complete: true,
            original_transport_context: context.clone(),
        };
        let common = crate::AdvisoryProviderReceiptObservation::from(&original);
        assert_eq!(common.original_transport_context, context);
        assert_eq!(common.response_payload, original.response_payload);
        assert_eq!(common.http_status, original.http_status);
        assert_eq!((common.input_tokens, common.output_tokens), (None, None));
        assert!(common.response_complete);
    }
}
