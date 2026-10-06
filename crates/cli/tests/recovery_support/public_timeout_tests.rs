use super::*;

#[tokio::test]
async fn public_send_timeout_poisons_and_reaps_before_return_then_rejects_retry() {
    // Below the wire cap, above the target platform's ordinary child-pipe
    // capacity. The controlled sleep child never reads its retained stdin.
    let params = json!({"padding":"x".repeat(wire::MAX_WIRE_BYTES - 256)});
    let frame = json!({"jsonrpc":"2.0","id":1,"method":"fixture","params":params.clone()});
    let encoded_bytes = wire::encode(&frame).unwrap().len();
    let mut fixture = child("exec /bin/sleep 60", true, true);
    let input = fixture.stdin.take().unwrap();
    let output = BufReader::new(fixture.stdout.take().unwrap());
    let mut client = Mcp {
        child: fixture,
        input,
        output,
        sequence: 0,
        native: "synthetic".into(),
        poisoned: false,
    };
    let first = client.send_result("fixture", params).await;
    let reaped_at_return = client.child.try_wait();
    let poisoned_at_return = client.poisoned;
    let sequence_at_return = client.sequence;
    let retry = client.send_result("must-not-execute", json!({})).await;
    let sequence_after_retry = client.sequence;
    let cleanup = kill_and_reap(&mut client.child).await;
    assert_eq!(cleanup, Ok(()));
    assert!(encoded_bytes < wire::MAX_WIRE_BYTES && encoded_bytes > 512 * 1024);
    assert_eq!(first.unwrap_err(), "MCP send deadline");
    assert!(poisoned_at_return);
    assert!(reaped_at_return.unwrap().is_some());
    assert_eq!(retry.unwrap_err(), "MCP connection poisoned");
    assert_eq!(sequence_at_return, sequence_after_retry);
}
#[tokio::test]
async fn public_exchange_timeout_poisons_and_reaps_before_return_then_rejects_retry() {
    // Child keeps stdout open but emits no response. This exercises the actual
    // public request path and its unchanged fifteen-second production deadline.
    let mut fixture = child("exec /bin/sleep 60", true, true);
    let input = fixture.stdin.take().unwrap();
    let output = BufReader::new(fixture.stdout.take().unwrap());
    let mut client = Mcp {
        child: fixture,
        input,
        output,
        sequence: 0,
        native: "synthetic".into(),
        poisoned: false,
    };
    let first = client.exchange_result("fixture", json!({})).await;
    let reaped_at_return = client.child.try_wait();
    let poisoned_at_return = client.poisoned;
    let sequence_at_return = client.sequence;
    let retry = client.exchange_result("must-not-execute", json!({})).await;
    let sequence_after_retry = client.sequence;
    let cleanup = kill_and_reap(&mut client.child).await;
    assert_eq!(cleanup, Ok(()));
    assert_eq!(first.unwrap_err(), "MCP request deadline");
    assert!(poisoned_at_return);
    assert!(reaped_at_return.unwrap().is_some());
    assert_eq!(retry.unwrap_err(), "MCP connection poisoned");
    assert_eq!(sequence_at_return, sequence_after_retry);
}
