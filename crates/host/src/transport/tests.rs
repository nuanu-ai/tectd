use super::*;
use serde::Deserialize;
use serde_json::json;
use tect_domain::HostAuth;
use uuid::Uuid;

fn context() -> RequestContext {
    RequestContext {
        auth: HostAuth {
            host_id: Uuid::new_v4(),
            credential: "0".repeat(64),
        },
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: "wire-version-test".into(),
    }
}

#[test]
fn malformed_matrix_verification_authenticates_as_verifier_route() {
    assert_eq!(
        invalid_request_auth("verify_matrix_task"),
        InvalidRequestAuth::MatrixVerifier
    );
    assert_eq!(
        invalid_request_auth("candidate_advisory_verify"),
        InvalidRequestAuth::CandidateAdvisory
    );
    assert!(!allows_verifier_invalid_request("record_matrix_task"));
    assert!(!allows_verifier_invalid_request(
        crate::api::INVALID_PUBLIC_CALL
    ));
}

#[tokio::test]
async fn malformed_matrix_verification_denies_owner_and_reaches_decode_error_for_verifier() {
    let owner = authenticate_invalid_request_with("verify_matrix_task", |kind| async move {
        assert_eq!(kind, InvalidRequestAuth::MatrixVerifier);
        Err(Error::Forbidden)
    })
    .await;
    assert!(matches!(
        owner,
        WireResponse::Error {
            error: Error::Forbidden
        }
    ));

    let verifier = authenticate_invalid_request_with("verify_matrix_task", |kind| async move {
        assert_eq!(kind, InvalidRequestAuth::MatrixVerifier);
        Ok(())
    })
    .await;
    assert!(matches!(
        verifier,
        WireResponse::Error {
            error: Error::InvalidArguments
        }
    ));
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyWireRequest {
    #[allow(dead_code)]
    context: RequestContext,
    #[allow(dead_code)]
    tool_name: String,
    #[allow(dead_code)]
    arguments: Value,
    #[allow(dead_code)]
    output_capacity: usize,
}

#[test]
fn new_daemon_rejects_old_or_wrong_wire_before_operation_decode() {
    let legacy = json!({
        "context":context(),"tool_name":"open_workspace","arguments":{},
        "output_capacity":MAX_FRAME_BYTES
    });
    let request: WireRequest = serde_json::from_value(legacy).unwrap();
    assert_eq!(
        validate_wire_version(&request),
        Err(Error::InvalidConfiguration)
    );

    let wrong = WireRequest {
        api_version: Some(crate::api::WIRE_API_VERSION + 1),
        context: context(),
        tool_name: "open_workspace".into(),
        arguments: json!({}),
        output_capacity: MAX_FRAME_BYTES,
    };
    assert_eq!(
        validate_wire_version(&wrong),
        Err(Error::InvalidConfiguration)
    );
}

#[test]
fn old_daemon_shape_rejects_new_bridge_request_before_operation_decode() {
    let current = WireRequest {
        api_version: Some(crate::api::WIRE_API_VERSION),
        context: context(),
        tool_name: "open_workspace".into(),
        arguments: json!({}),
        output_capacity: MAX_FRAME_BYTES,
    };
    let encoded = serde_json::to_value(current).unwrap();
    assert!(serde_json::from_value::<LegacyWireRequest>(encoded).is_err());
}

#[test]
fn matrix_card_query_crosses_versioned_wire_as_strict_read_invocation() {
    let task_id = Uuid::new_v4();
    let public = crate::api::decode_public_call(
        "query",
        json!({"route":"scope.advisory.card","params":{"task_id":task_id,"expected_task_revision":2,"detail":"full","card_id":"EM02-SCOPE@0.1"}}),
    ).unwrap();
    let request = WireRequest {
        api_version: Some(crate::api::WIRE_API_VERSION),
        context: context(),
        tool_name: public.name.into(),
        arguments: public.arguments,
        output_capacity: MAX_FRAME_BYTES,
    };
    let decoded: WireRequest = serde_json::from_slice(&encode_line(&request).unwrap()).unwrap();
    assert_eq!(validate_wire_version(&decoded), Ok(()));
    assert!(matches!(
        parse_invocation(&decoded.tool_name, decoded.arguments),
        Ok(Invocation::Advisory(crate::advisory_tools::AdvisoryInvocation::MatrixCard {
            task_id: parsed_id,
            expected_task_revision: 2,
            detail: crate::advisory_tools::MatrixCardDetail::Full,
            ..
        })) if parsed_id == task_id
    ));
}
