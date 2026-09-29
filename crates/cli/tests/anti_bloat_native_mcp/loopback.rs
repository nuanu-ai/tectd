//! Controlled no-network-provider response for the S04 one-shot code path.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(super) async fn respond_once(listener: TcpListener, expected: Vec<u8>, abstain: bool) {
    let (mut stream, _) =
        tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
            .await
            .unwrap()
            .unwrap();
    let mut received = Vec::new();
    let mut block = [0u8; 4096];
    let (body_start, body_len) = loop {
        let n = stream.read(&mut block).await.unwrap();
        assert!(n > 0);
        received.extend_from_slice(&block[..n]);
        assert!(received.len() <= 128 * 1024);
        if let Some(end) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&received[..end]).unwrap();
            assert!(headers.starts_with("POST /v1/systemone HTTP/1.1"));
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer s04-loopback-only")
            );
            let len = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|value| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            break (end + 4, len);
        }
    };
    while received.len() < body_start + body_len {
        let n = stream.read(&mut block).await.unwrap();
        assert!(n > 0);
        received.extend_from_slice(&block[..n]);
    }
    assert_eq!(
        format!(
            "{:x}",
            Sha256::digest(&received[body_start..body_start + body_len])
        ),
        format!("{:x}", Sha256::digest(&expected)),
        "loopback request differs from reviewed bytes"
    );
    let choice = if abstain { "ABSTAIN" } else { "R0" };
    let (ranked, abstained) = if abstain { (0.1, 0.9) } else { (0.9, 0.1) };
    let response = serde_json::to_vec(&json!({
        "model":MODEL,
        "answers":{"anti_bloat_order_v1":{
            "type":"choice","choice":choice,
            "probabilities":{"R0":ranked,"ABSTAIN":abstained},
            "confidence":0.9
        }},
        "usage":{"input_tokens":120,"output_tokens":30}
    }))
    .unwrap();
    stream
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stream.write_all(&response).await.unwrap();
    stream.shutdown().await.unwrap();
}
