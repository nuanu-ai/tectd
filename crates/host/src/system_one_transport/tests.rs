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

fn config(url: &str) -> SystemOneTransportConfig {
    SystemOneTransportConfig {
        endpoint: Url::parse(url).unwrap(),
        timeout: Duration::from_millis(300),
        maximum_request_bytes: 1024,
        maximum_response_bytes: 1024,
    }
}

#[tokio::test]
async fn postheader_truncated_body_retains_status_and_received_prefix() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 2048];
        assert!(stream.read(&mut request).unwrap() > 0);
        stream.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 100\r\nConnection: close\r\n\r\nprefix").unwrap();
        // Keep the complete chunk available before inducing a body read timeout.
        stream.flush().unwrap();
        thread::sleep(Duration::from_millis(350));
    });
    let transport = SystemOneTransport::new(config(&url), "local-test").unwrap();
    let response = transport.post_once(b"{}").await.unwrap();
    assert_eq!(response.status, 500);
    assert_eq!(response.body, b"prefix");
    assert!(!response.response_complete);
    assert_eq!(
        response
            .original_transport_context
            .provider_failure_code
            .as_deref(),
        Some("response-body-read")
    );
    assert_eq!(
        response.original_transport_context.outcome,
        AdvisoryDispatchOutcome::ProviderFailure
    );
    assert_eq!(
        response.original_transport_context.raw_response_ref,
        Some(format!("sha256:{:x}", Sha256::digest(b"prefix")))
    );
    handle.join().unwrap();
}

fn server(
    status: &str,
    body: &[u8],
    delay: Duration,
) -> (String, thread::JoinHandle<Vec<u8>>, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let observed = count.clone();
    let status = status.to_string();
    let body = body.to_vec();
    let handle = thread::spawn(move || {
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
    (url, handle, count)
}

#[tokio::test]
async fn sends_exact_bytes_and_sensitive_bearer_once_preserving_status_and_raw() {
    for status in ["200 OK", "500 Internal Server Error", "302 Found"] {
        let (url, handle, count) = server(status, b"malformed { bytes", Duration::ZERO);
        let transport = SystemOneTransport::new(config(&url), "local-test").unwrap();
        assert!(transport.authorization.is_sensitive());
        let exact = b"{ \"frozen\": true }\n";
        let response = transport.post_once(exact).await.unwrap();
        assert_eq!(response.status, status[..3].parse::<u16>().unwrap());
        assert_eq!(response.body, b"malformed { bytes");
        assert!(response.response_complete);
        assert_eq!(
            response.original_transport_context.send_certainty,
            AdvisorySendCertainty::Sent
        );
        assert_eq!(
            response.original_transport_context.outcome,
            if status.starts_with("200") {
                AdvisoryDispatchOutcome::ProviderResponse
            } else {
                AdvisoryDispatchOutcome::ProviderFailure
            }
        );
        assert_eq!(
            response.original_transport_context.raw_response_ref,
            Some(format!("sha256:{:x}", Sha256::digest(&response.body)))
        );
        let request = handle.join().unwrap();
        let end = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        assert_eq!(&request[end + 4..], exact);
        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
        assert!(headers.starts_with("post /v1/systemone http/1.1"));
        assert!(headers.contains("authorization: bearer local-test"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn rejects_invalid_configuration_and_credentials() {
    for url in [
        "http://localhost/v1/systemone",
        "http://example.com/v1/systemone",
        "https://example.com/other",
        "https://user@example.com/v1/systemone",
        "https://example.com/v1/systemone?q=1",
        "https://example.com/v1/systemone#x",
    ] {
        assert!(SystemOneTransport::new(config(url), "local-test").is_err());
    }
    for credential in ["", " ", "bad\nheader"] {
        assert!(
            SystemOneTransport::new(config("https://example.com/v1/systemone"), credential)
                .is_err()
        );
    }
    for bad in 0..3 {
        let mut c = config("https://example.com/v1/systemone");
        match bad {
            0 => c.timeout = Duration::ZERO,
            1 => c.maximum_request_bytes = 0,
            _ => c.maximum_response_bytes = 0,
        };
        assert!(SystemOneTransport::new(c, "local-test").is_err());
    }
    assert!(SystemOneTransport::new(config("http://[::1]/v1/systemone"), "local-test").is_ok());
}

#[tokio::test]
async fn byte_bounds_capture_actual_prefix_and_preheader_timeout_remains_error() {
    let transport =
        SystemOneTransport::new(config("http://127.0.0.1:1/v1/systemone"), "local-test").unwrap();
    assert!(matches!(
        transport.post_once(&vec![0; 1025]).await,
        Err(Error::RequestTooLarge)
    ));
    assert!(matches!(
        transport.post_once(b"").await,
        Err(Error::RequestTooLarge)
    ));
    let (url, handle, count) = server("200 OK", b"123456", Duration::ZERO);
    let mut c = config(&url);
    c.maximum_response_bytes = 5;
    let t = SystemOneTransport::new(c, "local-test").unwrap();
    let response = t.post_once(b"{}").await.unwrap();
    assert_eq!(response.body, b"12345");
    assert_eq!(response.status, 200);
    assert!(!response.response_complete);
    assert_eq!(
        response
            .original_transport_context
            .provider_failure_code
            .as_deref(),
        Some("response-oversize")
    );
    assert_eq!(
        response.original_transport_context.raw_response_ref,
        Some(format!("sha256:{:x}", Sha256::digest(b"12345")))
    );
    handle.join().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let (url, handle, count) = server("200 OK", b"{}", Duration::from_millis(100));
    let mut c = config(&url);
    c.timeout = Duration::from_millis(20);
    let t = SystemOneTransport::new(c, "local-test").unwrap();
    assert!(matches!(
        t.post_once(b"{}").await,
        Err(Error::TransportUnavailable)
    ));
    handle.join().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn empty_response_is_preserved_as_complete_raw_evidence() {
    let (url, handle, count) = server("204 No Content", b"", Duration::ZERO);
    let transport = SystemOneTransport::new(config(&url), "local-test").unwrap();
    let response = transport.post_once(b"{}").await.unwrap();
    assert_eq!(response.status, 204);
    assert!(response.body.is_empty());
    assert!(response.response_complete);
    handle.join().unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
