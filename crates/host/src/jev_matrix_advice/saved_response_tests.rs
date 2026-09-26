use super::*;
use serde_json::json;
use tect_application::MatrixProviderBinding;
use tect_domain::{
    AdvisoryDispatch, AdvisoryDispatchOutcome, AdvisoryDispatchState, AdvisoryModelConfiguration,
    AdvisoryProviderProfileRef, AdvisoryRetryBasis, AdvisorySendCertainty,
};
use uuid::Uuid;

pub(super) fn stored_dispatch(
    request_payload: Vec<u8>,
    response_payload: Option<Vec<u8>>,
    configuration_snapshot: serde_json::Value,
) -> StoredMatrixDispatch {
    let id = Uuid::from_u128(1);
    StoredMatrixDispatch {
        dispatch: AdvisoryDispatch {
            id,
            opportunity_id: Uuid::from_u128(2),
            predecessor_dispatch_id: None,
            attempt_number: 1,
            provider: "profile".into(),
            model: "model".into(),
            configuration_digest: String::new(),
            material_digest: String::new(),
            payload_digest: String::new(),
            input_tokens: None,
            output_tokens: None,
            latency_ms: None,
            state: AdvisoryDispatchState::Sealed,
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: Some(AdvisoryDispatchOutcome::ProviderResponse),
            retry_basis: AdvisoryRetryBasis::Initial,
            raw_response_ref: None,
        },
        binding: MatrixProviderBinding {
            task_id: Uuid::from_u128(3),
            task_revision: 1,
            input_digest: String::new(),
            choice_set_id: String::new(),
            choice_set_version: 1,
            choice_set_digest: String::new(),
            evaluation_digest: String::new(),
            verification_digest: Some(String::new()),
        },
        provider_profile_ref: AdvisoryProviderProfileRef {
            id: "profile".into(),
        },
        model_configuration: AdvisoryModelConfiguration {
            model: "model".into(),
        },
        configuration_snapshot,
        destination: "test-destination".into(),
        wire_version: WIRE_VERSION.into(),
        request_payload,
        request_payload_sha256: String::new(),
        response_payload,
        response_payload_sha256: Some(String::new()),
        response_complete: true,
        response_http_status: None,
        original_input_tokens: None,
        original_output_tokens: None,
        original_elapsed_ms: None,
        raw_observation_sealed: false,
    }
}

fn identity() -> MatrixProviderIdentity {
    MatrixProviderIdentity {
        provider_profile_ref: AdvisoryProviderProfileRef {
            id: "profile".into(),
        },
        model_configuration: AdvisoryModelConfiguration {
            model: "model".into(),
        },
        destination: "test-destination".into(),
        wire_version: WIRE_VERSION.into(),
    }
}

#[test]
fn oversized_public_saved_payloads_fail_before_configuration_inspection() {
    let malformed_configuration = json!({"unexpected": "bounded but untrusted"});
    let request_oversized = stored_dispatch(
        vec![b'x'; 9],
        Some(vec![b'y']),
        malformed_configuration.clone(),
    );
    assert_eq!(
        validate_saved_dispatch_bounds(&request_oversized, &identity(), 8, 8),
        Err(Error::RequestTooLarge)
    );

    let response_oversized =
        stored_dispatch(vec![b'x'], Some(vec![b'y'; 9]), malformed_configuration);
    assert_eq!(
        validate_saved_dispatch_bounds(&response_oversized, &identity(), 8, 8),
        Err(Error::RequestTooLarge)
    );
}
