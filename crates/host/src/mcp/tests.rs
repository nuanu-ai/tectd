use super::*;
use tect_domain::HostAuth;
use uuid::Uuid;

fn synthetic_session() -> McpSession {
    McpSession {
        socket: PathBuf::from("/__tect_test__/unopened-test-socket"),
        context: HostContext::new(
            HostAuth {
                host_id: Uuid::new_v4(),
                credential: "0".repeat(64),
            },
            "synthetic-unit-fixture".into(),
        )
        .unwrap(),
        lifecycle: Lifecycle::New,
    }
}

#[test]
fn tool_errors_have_one_json_content_and_a_short_intro() {
    let result = failed_tool_result(Error::Unauthorized);
    assert_eq!(result["isError"], true);
    assert!(result.get("structuredContent").is_none());
    let data: Value = serde_json::from_str(result["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(data["error"]["code"], "unauthorized");
    assert!(data["actions"].as_array().unwrap().is_empty());
    assert!(result["content"][0]["text"].as_str().unwrap().len() <= 2000);
}

#[tokio::test]
async fn initialization_negotiates_the_single_supported_protocol() {
    for requested in [SERVER_PROTOCOL, "2025-03-26"] {
        let mut session = synthetic_session();
        let response = session
            .handle_value(json!({
                "jsonrpc": "2.0",
                "id": "init",
                "method": "initialize",
                "params": {
                    "protocolVersion": requested,
                    "capabilities": {},
                    "clientInfo": {"name": "synthetic-test", "version": "1"}
                }
            }))
            .await
            .unwrap();
        assert_eq!(response["result"]["protocolVersion"], SERVER_PROTOCOL);
        assert_eq!(response["result"]["serverInfo"]["name"], "tectd-mcp");
        assert_eq!(response["result"]["serverInfo"]["title"], "TectD MCP");
        assert_eq!(session.lifecycle, Lifecycle::AwaitingInitialized);
    }
}

#[tokio::test]
async fn initialize_requires_capabilities_and_client_identity_objects() {
    for params in [
        json!({"protocolVersion": SERVER_PROTOCOL}),
        json!({
            "protocolVersion": SERVER_PROTOCOL,
            "capabilities": [],
            "clientInfo": {"name": "synthetic-test", "version": "1"}
        }),
        json!({
            "protocolVersion": SERVER_PROTOCOL,
            "capabilities": {},
            "clientInfo": {"name": "synthetic-test", "version": 1}
        }),
    ] {
        let mut session = synthetic_session();
        let response = session
            .handle_value(json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": params
            }))
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(session.lifecycle, Lifecycle::New);
    }
}

#[tokio::test]
async fn ping_is_available_before_initialization() {
    let mut session = synthetic_session();
    let response = session
        .handle_value(json!({"jsonrpc": "2.0", "id": 7, "method": "ping"}))
        .await
        .unwrap();
    assert_eq!(response["result"], json!({}));
    assert_eq!(session.lifecycle, Lifecycle::New);
}

#[tokio::test]
async fn codex_tool_discovery_accepts_standard_progress_metadata() {
    let mut session = synthetic_session();
    session.lifecycle = Lifecycle::Ready;
    let response = session
        .handle_value(json!({
            "jsonrpc": "2.0", "id": 8, "method": "tools/list",
            "params": {"_meta": {"progressToken": 0}}
        }))
        .await
        .unwrap();
    assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 17);
    for params in [
        json!({"_meta": null}),
        json!({"_meta": {"progressToken": false}}),
        json!({"native_session_id": Uuid::new_v4()}),
    ] {
        let response = session
            .handle_value(json!({
                "jsonrpc": "2.0", "id": 9, "method": "tools/list", "params": params
            }))
            .await
            .unwrap();
        assert_eq!(response["error"]["code"], -32602);
    }
}

#[tokio::test]
async fn object_without_method_is_an_invalid_request() {
    let mut session = synthetic_session();
    let response = session.handle_value(json!({})).await.unwrap();
    assert_eq!(response["id"], Value::Null);
    assert_eq!(response["error"]["code"], -32600);
}

#[test]
fn tool_call_params_accept_only_standard_meta_beside_name_and_arguments() {
    let params: ToolCallParams = serde_json::from_value(json!({
        "name": "get_state", "arguments": {},
        "_meta": {"progressToken": "p", "threadId": Uuid::new_v4()}
    }))
    .unwrap();
    assert_eq!(params.name, "get_state");
    assert!(
        serde_json::from_value::<ToolCallParams>(json!({
            "name": "get_state", "arguments": {}, "workspace_key": "spoofed"
        }))
        .is_err()
    );
}

#[test]
fn native_identity_is_derived_from_each_call_metadata() {
    let session = synthetic_session();
    let first = Uuid::new_v4().to_string();
    let second = Uuid::new_v4().to_string();
    let first_meta = json!({"threadId": first, "progressToken": "kept"});
    let second_meta = json!({"threadId": second});
    let first_context = request_context(&session.context, first_meta.as_object()).unwrap();
    let second_context = request_context(&session.context, second_meta.as_object()).unwrap();
    assert_eq!(first_context.native_session_id, first);
    assert_eq!(second_context.native_session_id, second);
    assert_ne!(
        first_context.native_session_id,
        second_context.native_session_id
    );
}

#[tokio::test]
async fn missing_or_invalid_thread_id_fails_before_daemon_transport() {
    let session = synthetic_session();
    for metadata in [
        None,
        Some(json!({})),
        Some(json!({"threadId": null})),
        Some(json!({"threadId": 7})),
        Some(json!({"threadId": ""})),
        Some(json!({"threadId": "not-a-uuid"})),
        Some(json!({"threadId": Uuid::nil()})),
        Some(json!({"threadId": "019D14D7-4678-7EE1-8000-000000000001"})),
    ] {
        let mut params = json!({"name": "get_state", "arguments": {}});
        if let Some(metadata) = metadata {
            params["_meta"] = metadata;
        }
        let response = session.tools_call(json!(1), Some(&params)).await;
        let data: Value =
            serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(data["error"]["code"], "invalid_native_session");
    }
}
