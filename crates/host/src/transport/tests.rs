use super::*;
use serde::Deserialize;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
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

#[tokio::test]
async fn daemon_deadline_is_typed_and_bridge_budget_has_write_margin() {
    assert_eq!(OPERATION_TIMEOUT, Duration::from_secs(15));
    assert_eq!(RESPONSE_READ_TIMEOUT, Duration::from_secs(25));
    assert!(RESPONSE_READ_TIMEOUT > OPERATION_TIMEOUT + IO_TIMEOUT);
    let expired = timeout(
        Duration::from_millis(1),
        std::future::pending::<Result<Value>>(),
    )
    .await;
    let response = operation_response(expired, MAX_FRAME_BYTES);
    let encoded = encode_line(&response).unwrap();
    assert_eq!(decode_wire_response(&encoded), Err(Error::OperationTimeout));
    assert_eq!(Error::OperationTimeout.code(), "operation_timeout");
}

#[tokio::test]
async fn delayed_daemon_response_preserves_wire_error_and_socket_absence_is_transport_error() {
    let directory = tempfile::Builder::new().tempdir_in("/private/tmp").unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.path().join("tectd.sock");
    assert_eq!(
        call_tool(&socket, &context(), "get_state", json!({})).await,
        Err(Error::TransportUnavailable)
    );

    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let daemon = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let (read, mut write) = tokio::io::split(stream);
        let mut reader = FrameReader::new(read);
        assert!(matches!(reader.next().await.unwrap(), Some(Frame::Data(_))));
        tokio::time::sleep(Duration::from_millis(30)).await;
        let bytes = encode_line(&WireResponse::Error {
            error: Error::OperationTimeout,
        })
        .unwrap();
        write.write_all(&bytes).await.unwrap();
    });
    assert_eq!(
        call_tool(&socket, &context(), "get_state", json!({})).await,
        Err(Error::OperationTimeout)
    );
    daemon.await.unwrap();
}
