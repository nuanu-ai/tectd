use super::*;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

const KEY: &str = "local-test-key";

pub(crate) struct FakeJev {
    pub url: String,
    /// Joins to the raw request the server read.
    pub request: thread::JoinHandle<Vec<u8>>,
    pub connections: Arc<AtomicUsize>,
}

/// A loopback server that answers one request after `delay` and counts every connection.
pub(crate) fn fake_jev(status: &str, body: &[u8], delay: Duration) -> FakeJev {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let connections = Arc::new(AtomicUsize::new(0));
    let observed = connections.clone();
    let status = status.to_string();
    let body = body.to_vec();
    let request = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        observed.fetch_add(1, Ordering::SeqCst);
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            assert!(n > 0);
            request.extend_from_slice(&chunk[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&request[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length: ")
                            .map(|s| s.parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= end + 4 + length {
                    break;
                }
            }
        }
        thread::sleep(delay);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: /redirected\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
        listener.set_nonblocking(true).unwrap();
        thread::sleep(Duration::from_millis(30));
        while listener.accept().is_ok() {
            observed.fetch_add(1, Ordering::SeqCst);
        }
        request
    });
    FakeJev {
        url,
        request,
        connections,
    }
}

fn transport(url: &str, timeout_ms: u64) -> JevTransport {
    JevTransport::new(
        validate_endpoint(url).unwrap(),
        Duration::from_millis(timeout_ms),
        KEY,
    )
    .unwrap()
}

#[test]
fn only_the_production_endpoint_and_numeric_loopback_are_accepted() {
    for accepted in [
        "https://api.typesafe.ai/v1/systemone",
        "http://127.0.0.1:8080/v1/systemone",
    ] {
        assert_eq!(validate_endpoint(accepted).unwrap().as_str(), accepted);
    }
    for refused in [
        "",
        "api.typesafe.ai/v1/systemone",
        "http://api.typesafe.ai/v1/systemone",
        "https://api.typesafe.ai:8443/v1/systemone",
        "https://api.typesafe.ai/v1/systemone/",
        "https://api.typesafe.ai/v1/other",
        "https://api.typesafe.ai/v1/systemone?q=1",
        "https://api.typesafe.ai/v1/systemone#x",
        "https://user@api.typesafe.ai/v1/systemone",
        "https://api.typesafe.ai.example.com/v1/systemone",
        "https://example.com/v1/systemone",
        "http://localhost:8080/v1/systemone",
        "http://127.0.0.1/v1/systemone",
        "http://127.0.0.2:8080/v1/systemone",
        "http://[::1]:8080/v1/systemone",
        "https://127.0.0.1:8080/v1/systemone",
        "http://127.0.0.1:8080/v1/systemone?q=1",
        "http://user:pass@127.0.0.1:8080/v1/systemone",
    ] {
        assert_eq!(
            validate_endpoint(refused).err(),
            Some(Error::InvalidConfiguration),
            "{refused}"
        );
    }
}

#[test]
fn unusable_keys_and_zero_timeout_are_refused_and_the_key_is_not_printed() {
    let endpoint = || validate_endpoint("https://api.typesafe.ai/v1/systemone").unwrap();
    for key in ["", " ", "bad\nheader"] {
        assert!(JevTransport::new(endpoint(), Duration::from_millis(100), key).is_err());
    }
    assert!(JevTransport::new(endpoint(), Duration::ZERO, KEY).is_err());
    let authorization = bearer(KEY).unwrap();
    assert!(authorization.is_sensitive());
    assert!(!format!("{authorization:?}").contains(KEY));
    assert_eq!(MAX_REQUEST_BYTES, 262_144);
    assert_eq!(MAX_RESPONSE_BYTES, 131_072);
}

#[tokio::test]
async fn sends_the_exact_bytes_and_bearer_once_and_follows_no_redirect() {
    for status in ["200 OK", "500 Internal Server Error", "302 Found"] {
        let server = fake_jev(status, b"malformed { bytes", Duration::ZERO);
        let exact = b"{ \"frozen\": true }\n";
        let result = transport(&server.url, 300).post_once(exact).await;
        match result {
            Ok(answer) => {
                assert_eq!(status, "200 OK");
                assert_eq!(answer.body, b"malformed { bytes");
                assert!(answer.latency_ms.is_some());
            }
            Err(failure) => {
                assert_ne!(status, "200 OK");
                assert_eq!(failure.reason, AdviceUnavailableReason::HttpStatus);
                assert_eq!(failure.send_certainty, AdviceSendCertainty::Sent);
                assert_eq!(
                    failure.detail,
                    format!("Jev answered with HTTP status {}", &status[..3])
                );
                assert_eq!(
                    failure.body.as_deref(),
                    Some(b"malformed { bytes".as_slice())
                );
                assert!(failure.latency_ms.is_some());
            }
        }
        let request = server.request.join().unwrap();
        let end = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        assert_eq!(&request[end + 4..], exact);
        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/systemone http/1.1"));
        assert!(headers.contains(&format!("authorization: bearer {KEY}")));
        assert!(headers.contains("content-type: application/json"));
        assert_eq!(server.connections.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn a_request_over_the_ceiling_is_never_sent() {
    let mut transport = transport("http://127.0.0.1:1/v1/systemone", 300);
    transport.request_ceiling = 1024;
    let failure = transport.post_once(&vec![b'x'; 1025]).await.err().unwrap();
    assert_eq!(failure.reason, AdviceUnavailableReason::RequestTooLarge);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::NotSent);
    assert_eq!(
        failure.detail,
        "the request of 1025 bytes exceeds the limit of 1024 bytes"
    );
    assert!(failure.body.is_none() && failure.latency_ms.is_none());
}

#[tokio::test]
async fn an_answer_over_the_ceiling_keeps_the_received_prefix() {
    let server = fake_jev("200 OK", b"123456", Duration::ZERO);
    let mut transport = transport(&server.url, 300);
    transport.response_ceiling = 5;
    let failure = transport.post_once(b"{}").await.err().unwrap();
    assert_eq!(failure.reason, AdviceUnavailableReason::InvalidResponse);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::Sent);
    assert_eq!(failure.detail, "the answer exceeds the limit of 5 bytes");
    assert_eq!(failure.body.as_deref(), Some(b"12345".as_slice()));
    server.request.join().unwrap();
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn silence_past_the_timeout_is_sent_unknown_and_is_not_retried() {
    let server = fake_jev("200 OK", b"{}", Duration::from_millis(150));
    let failure = transport(&server.url, 20)
        .post_once(b"{}")
        .await
        .err()
        .unwrap();
    assert_eq!(failure.reason, AdviceUnavailableReason::Timeout);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::SentUnknown);
    assert_eq!(failure.detail, "Jev did not answer within 20 ms");
    assert!(failure.body.is_none() && failure.latency_ms.is_some());
    server.request.join().unwrap();
    assert_eq!(server.connections.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_refused_connection_is_not_sent() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    drop(listener);
    let failure = transport(&url, 300).post_once(b"{}").await.err().unwrap();
    assert_eq!(failure.reason, AdviceUnavailableReason::TransportError);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::NotSent);
    assert!(failure.body.is_none() && failure.latency_ms.is_none());
}

#[tokio::test]
async fn an_answer_cut_after_its_headers_keeps_status_certainty_and_prefix() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 2048];
        assert!(stream.read(&mut request).unwrap() > 0);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nprefix")
            .unwrap();
        stream.flush().unwrap();
        // Hold the connection open past the client's deadline.
        thread::sleep(Duration::from_millis(350));
    });
    let failure = transport(&url, 300).post_once(b"{}").await.err().unwrap();
    assert_eq!(failure.reason, AdviceUnavailableReason::Timeout);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::Sent);
    assert_eq!(failure.body.as_deref(), Some(b"prefix".as_slice()));
    handle.join().unwrap();
}

#[tokio::test]
async fn an_empty_answer_is_complete_and_an_echoed_key_is_redacted() {
    let server = fake_jev("204 No Content", b"", Duration::ZERO);
    let answer = transport(&server.url, 300)
        .post_once(b"{}")
        .await
        .ok()
        .unwrap();
    assert!(answer.body.is_empty());
    server.request.join().unwrap();

    let echo = format!("{{\"error\":\"bad key {KEY}\",\"again\":\"{KEY}\"}}");
    let server = fake_jev("401 Unauthorized", echo.as_bytes(), Duration::ZERO);
    let failure = transport(&server.url, 300)
        .post_once(b"{}")
        .await
        .err()
        .unwrap();
    assert_eq!(
        failure.body.as_deref(),
        Some(br#"{"error":"bad key [redacted]","again":"[redacted]"}"#.as_slice())
    );
    assert!(!failure.detail.contains(KEY));
    server.request.join().unwrap();
}
