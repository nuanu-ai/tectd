use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncWrite, BufReader};

#[tokio::test]
async fn request_wire_limit_includes_newline_and_utf8_bytes() {
    assert_eq!(wire::MAX_WIRE_BYTES, 8 * 1024 * 1024 + 1);
    let overhead = 3; // Two string quotes and the reserved final LF.
    assert_eq!(wire::encode(&json!("")).unwrap().len(), overhead);
    let count = wire::MAX_WIRE_BYTES - overhead;
    let ascii = wire::encode(&json!("a".repeat(count))).unwrap();
    assert_eq!(ascii.len(), wire::MAX_WIRE_BYTES);
    assert_eq!(ascii.last(), Some(&b'\n'));
    assert!(wire::encode(&json!("a".repeat(count + 1))).is_err());
    let unicode = format!("{}{}", "β".repeat(count / 2), "a".repeat(count % 2));
    assert_eq!(
        wire::encode(&json!(unicode)).unwrap().len(),
        wire::MAX_WIRE_BYTES
    );
    assert!(wire::encode(&json!("β".repeat(count / 2 + 1))).is_err());

    // Quote, backslash, LF, NUL and beta encode to 2+2+2+6+2 = 14 bytes.
    assert_eq!(
        wire::encode(&json!("\"\\\n\0β")).unwrap(),
        b"\"\\\"\\\\\\n\\u0000\xce\xb2\"\n"
    );
    let escaped = format!(
        "{}{}",
        "\"\\\n\0β".repeat(count / 14),
        "a".repeat(count % 14)
    );
    assert_eq!(
        wire::encode(&json!(&escaped)).unwrap().len(),
        wire::MAX_WIRE_BYTES
    );
    let mut input = WriteCounter::default();
    let rejected = wire::send(&mut input, &json!(format!("{escaped}a"))).await;
    assert_eq!(
        rejected.unwrap_err(),
        "MCP request serialization or wire limit"
    );
    assert_eq!((input.writes, input.flushes), (0, 0));
}
async fn response(bytes: &[u8]) -> Result<Value, String> {
    wire::receive(&mut BufReader::new(bytes), 1).await
}
#[tokio::test]
async fn response_limit_includes_newline_and_preserves_unicode() {
    let base = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"\"}\n";
    let count = wire::MAX_WIRE_BYTES - base.len();
    let text = format!("{}{}", "β".repeat(count / 2), "a".repeat(count % 2));
    let frame = format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"{text}\"}}\n");
    let accepted = response(frame.as_bytes()).await;
    let overflow =
        response(format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"{text}a\"}}\n").as_bytes())
            .await;
    assert_eq!(frame.len(), wire::MAX_WIRE_BYTES);
    assert_eq!(accepted.unwrap()["result"], text);
    assert!(overflow.unwrap_err().starts_with("MCP response wire limit"));

    // Hand-built JSON escaping is independent of wire::encode/serde output.
    let encoded = format!(
        "{}{}",
        "\\\"\\\\\\n\\u0000β".repeat(count / 14),
        "a".repeat(count % 14)
    );
    let decoded = format!(
        "{}{}",
        "\"\\\n\0β".repeat(count / 14),
        "a".repeat(count % 14)
    );
    let escaped_frame = format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"{encoded}\"}}\n");
    assert_eq!(escaped_frame.len(), wire::MAX_WIRE_BYTES);
    // First fill is the whole JSON payload; the final LF is a separate fill.
    let mut fragmented =
        BufReader::with_capacity(wire::MAX_WIRE_BYTES - 1, escaped_frame.as_bytes());
    let escaped_result = wire::receive(&mut fragmented, 1).await;
    assert_eq!(escaped_result.unwrap()["result"], decoded);
    let overflow_frame = format!("{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":\"{encoded}a\"}}\n");
    let mut fragmented =
        BufReader::with_capacity(wire::MAX_WIRE_BYTES - 1, overflow_frame.as_bytes());
    let overflow_result = wire::receive(&mut fragmented, 1).await;
    assert_eq!(overflow_result.unwrap_err(), "MCP response wire limit");
    // Overflow fragment including LF was rejected before consume/extend.
    assert_eq!(
        fragmented.buffer(),
        &overflow_frame.as_bytes()[wire::MAX_WIRE_BYTES - 1..]
    );
    for (extra, expected) in [
        (0, "MCP response wire limit before newline"),
        (1, "MCP response wire limit"),
    ] {
        let unterminated = vec![b'x'; wire::MAX_WIRE_BYTES + extra];
        let mut fragmented =
            BufReader::with_capacity(wire::MAX_WIRE_BYTES - 1, unterminated.as_slice());
        let bounded = wire::receive(&mut fragmented, 1).await;
        assert_eq!(bounded.unwrap_err(), expected);
        if extra == 1 {
            assert_eq!(fragmented.buffer(), b"xx"); // rejected before consume/extend
        }
    }
}
#[tokio::test]
async fn incomplete_and_invalid_responses_return_fixed_phase_errors() {
    for (bytes, expected) in [
        (&b""[..], "MCP response EOF before newline"),
        (&b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}"[..], "MCP response EOF before newline"),
        (&b"\xff\n"[..], "MCP response UTF-8"),
        (&b"secret-invalid-json\n"[..], "MCP response JSON"),
        (&b"{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}\n"[..], "MCP response envelope or id"),
        (&b"{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":null}\n"[..], "MCP response result/error shape"),
        (&b"{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":[]}\n"[..], "MCP response result/error shape"),
        (&b"{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":\"wrong\",\"message\":\"secret\"}}\n"[..], "MCP response result/error shape"),
        (&b"{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32601,\"message\":7}}\n"[..], "MCP response result/error shape"),
        (&b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{},\"error\":{}}\n"[..], "MCP response result/error shape"),
        (&b"{\"jsonrpc\":\"2.0\",\"id\":1}\n"[..], "MCP response result/error shape"),
    ] {
        assert_eq!(response(bytes).await.unwrap_err(), expected);
    }
}
#[tokio::test]
async fn valid_matching_json_rpc_error_is_a_complete_response() {
    let frame = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32601,\"message\":\"method_not_found\"}}\n";
    let result = response(frame).await;
    assert_eq!(result.unwrap()["error"]["code"], -32601);
}

#[derive(Default)]
struct WriteCounter {
    writes: usize,
    flushes: usize,
}
impl AsyncWrite for WriteCounter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        self.writes += 1;
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        self.flushes += 1;
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct PendingWrite {
    flush: bool,
}
impl AsyncWrite for PendingWrite {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.flush {
            Poll::Ready(Ok(bytes.len()))
        } else {
            Poll::Pending
        }
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
}
#[tokio::test]
async fn whole_request_deadline_covers_pending_write_flush_and_read() {
    let message = json!({"jsonrpc":"2.0","id":1,"method":"fixture"});
    for flush in [false, true] {
        let mut input = PendingWrite { flush };
        let mut output = BufReader::new(&b""[..]);
        let result = tokio::time::timeout(
            Duration::from_millis(20),
            wire::exchange(&mut input, &mut output, &message, 1),
        )
        .await;
        assert!(result.is_err());
    }
    let (reader, peer) = tokio::io::duplex(64);
    let mut output = BufReader::new(reader);
    let result =
        tokio::time::timeout(Duration::from_millis(20), wire::receive(&mut output, 1)).await;
    drop(peer);
    assert!(result.is_err());
}
