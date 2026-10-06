use serde_json::Value;
use std::{io::Write, time::Duration};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

// Match host frame::MAX_FRAME_BYTES (8 MiB JSON payload) plus its emitted LF.
pub(super) const MAX_WIRE_BYTES: usize = 8 * 1024 * 1024 + 1;
pub(super) const REQUEST_DEADLINE: Duration = Duration::from_secs(15);

// Reserve the newline before producing any JSON bytes. Never grow beyond cap.
struct WireBuffer(Vec<u8>);
impl Write for WireBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > (MAX_WIRE_BYTES - 1).saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("MCP request wire limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn encode(message: &Value) -> Result<Vec<u8>, String> {
    let mut buffer = WireBuffer(Vec::new());
    serde_json::to_writer(&mut buffer, message)
        .map_err(|_| "MCP request serialization or wire limit".to_owned())?;
    buffer.0.push(b'\n');
    Ok(buffer.0)
}
pub(super) async fn send<I: AsyncWrite + Unpin>(
    input: &mut I,
    message: &Value,
) -> Result<(), String> {
    let wire = encode(message)?;
    input
        .write_all(&wire)
        .await
        .map_err(|_| "MCP request write".to_owned())?;
    input
        .flush()
        .await
        .map_err(|_| "MCP request flush".to_owned())
}
pub(super) async fn receive<O: AsyncBufRead + Unpin>(
    output: &mut O,
    id: u64,
) -> Result<Value, String> {
    let mut frame = Vec::new();
    loop {
        let available = output
            .fill_buf()
            .await
            .map_err(|_| "MCP response read".to_owned())?;
        if available.is_empty() {
            return Err("MCP response EOF before newline".into());
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(available.len(), |i| i + 1);
        if count > MAX_WIRE_BYTES.saturating_sub(frame.len()) {
            return Err("MCP response wire limit".into());
        }
        let complete = available[count - 1] == b'\n';
        frame.extend_from_slice(&available[..count]);
        output.consume(count);
        if complete {
            break;
        }
        if frame.len() == MAX_WIRE_BYTES {
            return Err("MCP response wire limit before newline".into());
        }
    }
    let text = std::str::from_utf8(&frame).map_err(|_| "MCP response UTF-8".to_owned())?;
    let response: Value = serde_json::from_str(text).map_err(|_| "MCP response JSON".to_owned())?;
    if !response.is_object() || response["jsonrpc"] != "2.0" || response["id"].as_u64() != Some(id)
    {
        return Err("MCP response envelope or id".into());
    }
    match (response.get("result"), response.get("error")) {
        (Some(_), None) => Ok(response),
        (None, Some(error))
            if error.is_object()
                && error["code"].as_i64().is_some()
                && error["message"].is_string() =>
        {
            Ok(response)
        }
        _ => Err("MCP response result/error shape".into()),
    }
}
pub(super) async fn exchange<I: AsyncWrite + Unpin, O: AsyncBufRead + Unpin>(
    input: &mut I,
    output: &mut O,
    message: &Value,
    id: u64,
) -> Result<Value, String> {
    send(input, message).await?;
    receive(output, id).await
}
