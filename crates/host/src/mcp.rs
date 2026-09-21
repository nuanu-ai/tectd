use crate::Result;
use crate::context::HostContext;
use crate::frame::{Frame, FrameReader, MAX_FRAME_BYTES};
use crate::responses;
use crate::transport::call_tool_bounded;
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
    metadata: Option<Map<String, Value>>,
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

pub async fn run_stdio(socket: &Path, context: HostContext) -> Result<()> {
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
    context: HostContext,
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
                if control_params(params) {
                    Some(success_response(id, json!({})))
                } else {
                    Some(error_response(id, -32602, "invalid_params"))
                }
            }
            "tools/list" if self.lifecycle == Lifecycle::Ready => {
                if control_params(params) {
                    Some(success_response(id, crate::api::definitions()))
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
                "serverInfo": {
                    "name": "tectd-mcp", "title": "TectD MCP",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ))
    }

    fn handle_notification(&mut self, method: &str, params: Option<&Value>) {
        if method == "notifications/initialized"
            && self.lifecycle == Lifecycle::AwaitingInitialized
            && control_params(params)
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
        let context = match request_context(&self.context, params.metadata.as_ref()) {
            Ok(context) => context,
            Err(error) => return success_response(id, failed_tool_result(error)),
        };
        let envelope_bytes = serde_json::to_vec(&success_response(id.clone(), Value::Null))
            .expect("JSON response")
            .len()
            - 4;
        let capacity = MAX_FRAME_BYTES.saturating_sub(envelope_bytes);
        let routed = crate::api::decode_public_call(&params.name, params.arguments.clone());
        let public_decode_failed = routed.is_err();
        let public_decode_error = routed.as_ref().err().cloned();
        let (name, arguments) = match routed {
            Ok(call) => (call.name, call.arguments),
            Err(_) => (crate::api::INVALID_PUBLIC_CALL, Value::Object(Map::new())),
        };
        match call_tool_bounded(&self.socket, &context, name, arguments.clone(), capacity).await {
            Ok(result) => success_response(id, successful_tool_result(result)),
            Err(error) => {
                let error = public_decode_error.unwrap_or(error);
                let (failure_name, failure_arguments) = if public_decode_failed {
                    (params.name.as_str(), &params.arguments)
                } else {
                    (name, &arguments)
                };
                success_response(
                    id,
                    crate::setup_recovery::response(
                        error,
                        failure_name,
                        failure_arguments,
                        &self.socket,
                        &context,
                        capacity,
                    )
                    .await,
                )
            }
        }
    }
}

fn request_context(
    host: &HostContext,
    metadata: Option<&Map<String, Value>>,
) -> Result<RequestContext> {
    let thread_id = metadata
        .and_then(|metadata| metadata.get("threadId"))
        .and_then(Value::as_str)
        .ok_or(Error::InvalidNativeSession)?;
    host.request_context(thread_id)
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

fn control_params(params: Option<&Value>) -> bool {
    match params {
        None => true,
        Some(Value::Object(object)) => object.iter().all(|(key, value)| {
            key == "_meta"
                && value.as_object().is_some_and(|metadata| {
                    metadata
                        .get("progressToken")
                        .is_none_or(|token| token.is_string() || token.is_number())
                })
        }),
        _ => false,
    }
}

fn empty_arguments() -> Value {
    Value::Object(Map::new())
}

fn successful_tool_result(structured: Value) -> Value {
    responses::success(structured)
}

fn failed_tool_result(error: Error) -> Value {
    responses::failure(error, None)
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
mod tests;
