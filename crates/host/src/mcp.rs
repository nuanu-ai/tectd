use crate::Result;
use crate::frame::{Frame, FrameReader, MAX_FRAME_BYTES};
use crate::tools::definitions;
use crate::transport::call_tool;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::path::{Path, PathBuf};
use tect_domain::{Error, RequestContext};
use tokio::io::{AsyncWrite, AsyncWriteExt};

const SERVER_PROTOCOL: &str = "2025-06-18";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lifecycle {
    New,
    AwaitingInitialized,
    Ready,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ToolCallParams {
    name: String,
    #[serde(default = "empty_arguments")]
    arguments: Value,
    #[serde(default, rename = "_meta")]
    _meta: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitializeParams {
    protocol_version: String,
    #[serde(rename = "capabilities")]
    _capabilities: Map<String, Value>,
    client_info: ClientInfo,
    #[serde(default, rename = "_meta")]
    _meta: Option<Map<String, Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientInfo {
    name: String,
    version: String,
    #[serde(default, rename = "title")]
    _title: Option<String>,
}

pub async fn run_stdio(socket: &Path, context: RequestContext) -> Result<()> {
    if !socket.is_absolute() {
        return Err(Error::InvalidConfiguration);
    }
    let mut session = McpSession {
        socket: socket.to_owned(),
        context,
        lifecycle: Lifecycle::New,
    };
    let mut reader = FrameReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();

    while let Some(frame) = reader
        .next()
        .await
        .map_err(|_| Error::TransportUnavailable)?
    {
        let response = match frame {
            Frame::Data(bytes) => session.handle_bytes(&bytes).await,
            Frame::TooLarge => Some(error_response(
                Value::Null,
                -32600,
                Error::RequestTooLarge.code(),
            )),
        };
        if let Some(response) = response {
            write_json_line(&mut stdout, &response).await?;
        }
    }
    Ok(())
}

struct McpSession {
    socket: PathBuf,
    context: RequestContext,
    lifecycle: Lifecycle,
}

impl McpSession {
    async fn handle_bytes(&mut self, bytes: &[u8]) -> Option<Value> {
        let value = match serde_json::from_slice::<Value>(bytes) {
            Ok(value) => value,
            Err(_) => return Some(error_response(Value::Null, -32700, "parse_error")),
        };
        self.handle_value(value).await
    }

    async fn handle_value(&mut self, value: Value) -> Option<Value> {
        let object = match value.as_object() {
            Some(object) => object,
            None => return Some(error_response(Value::Null, -32600, "invalid_request")),
        };
        let method = match object.get("method").and_then(Value::as_str) {
            Some(method) => method,
            None => return Some(error_response(Value::Null, -32600, "invalid_request")),
        };
        if !valid_request_members(object)
            || object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        {
            return notification_aware_error(object, -32600, "invalid_request");
        }
        let id = match request_id(object) {
            Ok(id) => id,
            Err(()) => return Some(error_response(Value::Null, -32600, "invalid_request")),
        };
        let params = object.get("params");

        if id.is_none() {
            self.handle_notification(method, params);
            return None;
        }
        let id = id.expect("checked above");
        match method {
            "initialize" => self.initialize(id, params),
            "ping" => {
                if empty_params(params) {
                    Some(success_response(id, json!({})))
                } else {
                    Some(error_response(id, -32602, "invalid_params"))
                }
            }
            "tools/list" if self.lifecycle == Lifecycle::Ready => {
                if empty_params(params) {
                    Some(success_response(id, definitions()))
                } else {
                    Some(error_response(id, -32602, "invalid_params"))
                }
            }
            "tools/call" if self.lifecycle == Lifecycle::Ready => {
                Some(self.tools_call(id, params).await)
            }
            "tools/list" | "tools/call" => Some(error_response(id, -32600, "invalid_lifecycle")),
            _ => Some(error_response(id, -32601, "method_not_found")),
        }
    }

    fn initialize(&mut self, id: Value, params: Option<&Value>) -> Option<Value> {
        if self.lifecycle != Lifecycle::New {
            return Some(error_response(id, -32600, "invalid_lifecycle"));
        }
        let params = match params
            .cloned()
            .and_then(|params| serde_json::from_value::<InitializeParams>(params).ok())
        {
            Some(params)
                if !params.protocol_version.is_empty()
                    && !params.client_info.name.is_empty()
                    && !params.client_info.version.is_empty() =>
            {
                params
            }
            _ => return Some(error_response(id, -32602, "invalid_params")),
        };
        let protocol = if params.protocol_version == SERVER_PROTOCOL {
            params.protocol_version.as_str()
        } else {
            SERVER_PROTOCOL
        };
        self.lifecycle = Lifecycle::AwaitingInitialized;
        Some(success_response(
            id,
            json!({
                "protocolVersion": protocol,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "tect-mcp", "version": env!("CARGO_PKG_VERSION")}
            }),
        ))
    }

    fn handle_notification(&mut self, method: &str, params: Option<&Value>) {
        if method == "notifications/initialized"
            && self.lifecycle == Lifecycle::AwaitingInitialized
            && empty_params(params)
        {
            self.lifecycle = Lifecycle::Ready;
        }
    }

    async fn tools_call(&self, id: Value, params: Option<&Value>) -> Value {
        let params = match params
            .cloned()
            .and_then(|params| serde_json::from_value::<ToolCallParams>(params).ok())
        {
            Some(params) => params,
            None => return error_response(id, -32602, "invalid_params"),
        };
        match call_tool(&self.socket, &self.context, &params.name, params.arguments).await {
            Ok(result) => success_response(id, successful_tool_result(result)),
            Err(error) => success_response(id, failed_tool_result(error)),
        }
    }
}

fn valid_request_members(object: &Map<String, Value>) -> bool {
    object
        .keys()
        .all(|key| matches!(key.as_str(), "jsonrpc" | "id" | "method" | "params"))
}

fn request_id(object: &Map<String, Value>) -> std::result::Result<Option<Value>, ()> {
    match object.get("id") {
        None => Ok(None),
        Some(value) if value.is_string() || value.is_number() => Ok(Some(value.clone())),
        Some(_) => Err(()),
    }
}

fn notification_aware_error(
    object: &Map<String, Value>,
    code: i64,
    message: &'static str,
) -> Option<Value> {
    match request_id(object) {
        Ok(None) => None,
        Ok(Some(id)) => Some(error_response(id, code, message)),
        Err(()) => Some(error_response(Value::Null, code, message)),
    }
}

fn empty_params(params: Option<&Value>) -> bool {
    match params {
        None => true,
        Some(Value::Object(object)) => object.is_empty(),
        _ => false,
    }
}

fn empty_arguments() -> Value {
    Value::Object(Map::new())
}

fn successful_tool_result(structured: Value) -> Value {
    let text = serde_json::to_string(&structured).unwrap_or_else(|_| "{}".to_owned());
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": false
    })
}

fn failed_tool_result(error: Error) -> Value {
    let structured = json!({"error": {"code": error.code()}});
    let text = serde_json::to_string(&structured)
        .unwrap_or_else(|_| "{\"error\":{\"code\":\"transport_unavailable\"}}".to_owned());
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": true
    })
}

fn success_response(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error_response(id: Value, code: i64, message: &'static str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

async fn write_json_line<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| Error::TransportUnavailable)?;
    if bytes.len() > MAX_FRAME_BYTES {
        bytes = serde_json::to_vec(&error_response(
            Value::Null,
            -32603,
            Error::RequestTooLarge.code(),
        ))
        .map_err(|_| Error::TransportUnavailable)?;
    }
    bytes.push(b'\n');
    writer
        .write_all(&bytes)
        .await
        .map_err(|_| Error::TransportUnavailable)?;
    writer
        .flush()
        .await
        .map_err(|_| Error::TransportUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::HostAuth;
    use uuid::Uuid;

    fn synthetic_session() -> McpSession {
        McpSession {
            socket: PathBuf::from("/__tect_test__/unopened-test-socket"),
            context: RequestContext {
                auth: HostAuth {
                    host_id: Uuid::new_v4(),
                    credential: "0".repeat(64),
                },
                native_session_id: Uuid::new_v4().to_string(),
                workspace_key: "synthetic-unit-fixture".into(),
            },
            lifecycle: Lifecycle::New,
        }
    }

    #[test]
    fn tool_errors_have_stable_structured_and_text_content() {
        let result = failed_tool_result(Error::Unauthorized);
        assert_eq!(result["isError"], true);
        assert_eq!(result["structuredContent"]["error"]["code"], "unauthorized");
        assert_eq!(
            serde_json::from_str::<Value>(result["content"][0]["text"].as_str().unwrap()).unwrap(),
            result["structuredContent"]
        );
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
    async fn object_without_method_is_an_invalid_request() {
        let mut session = synthetic_session();
        let response = session.handle_value(json!({})).await.unwrap();
        assert_eq!(response["id"], Value::Null);
        assert_eq!(response["error"]["code"], -32600);
    }

    #[test]
    fn tool_call_params_accept_only_standard_meta_beside_name_and_arguments() {
        let params: ToolCallParams = serde_json::from_value(json!({
            "name": "get_state", "arguments": {}, "_meta": {"progressToken": "p"}
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
}
