use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tect_application::ScopeAdviceProviderError;
use tect_domain::{AdvisoryDispatchOutcome, AdvisorySendCertainty};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

struct FixtureResponse {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    delay: Duration,
}

async fn fixture(
    response: FixtureResponse,
) -> (
    Url,
    Arc<AtomicUsize>,
    oneshot::Receiver<Vec<u8>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let server_calls = calls.clone();
    let (sender, receiver) = oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        server_calls.fetch_add(1, Ordering::SeqCst);
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let count = socket.read(&mut buffer).await.unwrap();
            if count == 0 {
                return;
            }
            request.extend_from_slice(&buffer[..count]);
            if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while request.len() < header_end + length {
            let count = socket.read(&mut buffer).await.unwrap();
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
        }
        let _ = sender.send(request);
        tokio::time::sleep(response.delay).await;
        let reason = if response.status == 200 {
            "OK"
        } else {
            "ERROR"
        };
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.status,
            reason,
            response.content_type,
            response.body.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&response.body).await.unwrap();
    });
    (
        Url::parse(&format!("http://{address}/v1/systemone")).unwrap(),
        calls,
        receiver,
        task,
    )
}

fn provider(endpoint: Url, timeout: Duration, maximum: usize) -> JevScopeAdviceProvider {
    provider_with_caps(endpoint, timeout, usize::MAX, maximum)
}

fn provider_with_caps(
    endpoint: Url,
    timeout: Duration,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
) -> JevScopeAdviceProvider {
    JevScopeAdviceProvider::new(
        JevScopeAdviceConfig {
            profile: "fixture".into(),
            endpoint,
            model: "jev-1.13.0".into(),
            timeout,
            maximum_request_bytes,
            maximum_response_bytes,
        },
        "secret-fixture-credential".into(),
    )
    .unwrap()
}

#[tokio::test]
async fn serialized_request_at_cap_is_sent_once() {
    let request = request();
    let payload = serialize_request("jev-1.13.0", &request, &[]).unwrap();
    let (endpoint, calls, captured, server) = fixture(FixtureResponse {
        status: 200,
        content_type: "application/json",
        body: serde_json::to_vec(&valid_response()).unwrap(),
        delay: Duration::ZERO,
    })
    .await;
    let observation = provider_with_caps(endpoint, Duration::from_secs(1), payload.len(), 16_384)
        .attempt_request(DISPATCH_ID, &request)
        .await
        .unwrap();
    server.await.unwrap();
    let captured = captured.await.unwrap();
    let body_start = captured
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    assert_eq!(&captured[body_start..], payload);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(observation.send_certainty, AdvisorySendCertainty::Sent);
}

#[tokio::test]
async fn prepared_entity_is_the_received_entity_with_exact_digest_and_binding() {
    let (endpoint, calls, captured, server) = fixture(FixtureResponse {
        status: 200,
        content_type: "application/json",
        body: serde_json::to_vec(&valid_response()).unwrap(),
        delay: Duration::ZERO,
    })
    .await;
    let provider = provider(endpoint.clone(), Duration::from_secs(1), 16_384);
    let request = request();
    let prepared = provider.prepare(&request).unwrap();
    let expected_body = prepared.body().to_vec();
    let expected_digest = prepared.body_sha256().to_owned();
    assert_eq!(prepared.body_length(), expected_body.len());
    assert_eq!(
        expected_digest,
        format!("{:x}", Sha256::digest(&expected_body))
    );
    assert_eq!(prepared.profile(), "fixture");
    assert_eq!(prepared.model(), "jev-1.13.0");
    assert_eq!(prepared.wire_version(), "jev-system-one-json/2");
    assert_eq!(prepared.destination(), endpoint.as_str());

    let observation = provider
        .test_observation(DISPATCH_ID, prepared)
        .await
        .unwrap();
    server.await.unwrap();
    let captured = captured.await.unwrap();
    let body_start = captured
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    assert_eq!(&captured[body_start..], expected_body);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        observation.outcome,
        AdvisoryDispatchOutcome::ProviderResponse
    );
}

#[tokio::test]
async fn prepared_digest_changes_with_model_or_content_and_mismatch_is_not_sent() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = Url::parse(&format!("http://{address}/v1/systemone")).unwrap();
    let provider = provider(endpoint.clone(), Duration::from_secs(1), 16_384);
    let original = provider.prepare(&request()).unwrap();
    let legacy_label = PreparedScopeAdviceAttempt::new(
        request(),
        original.body().to_vec(),
        "fixture".into(),
        "jev-1.13.0".into(),
        endpoint.as_str().into(),
        "jev-system-one-json/1".into(),
    )
    .unwrap();
    assert_eq!(legacy_label.wire_version(), "jev-system-one-json/1");
    assert_eq!(
        provider.test_observation(DISPATCH_ID, legacy_label).await,
        Err(ScopeAdviceProviderError::ProvenNotSent)
    );
    let mut modified = request();
    modified.alternatives[0]
        .covered_obligation_ids
        .push("second-obligation".into());
    let changed_content = provider.prepare(&modified).unwrap();
    assert_ne!(original.body_sha256(), changed_content.body_sha256());

    let other_model = JevScopeAdviceProvider::new(
        JevScopeAdviceConfig {
            profile: "fixture".into(),
            endpoint,
            model: "jev-2".into(),
            timeout: Duration::from_secs(1),
            maximum_request_bytes: 16_384,
            maximum_response_bytes: 16_384,
        },
        "secret-fixture-credential".into(),
    )
    .unwrap();
    let changed_model = other_model.prepare(&request()).unwrap();
    assert_ne!(original.body_sha256(), changed_model.body_sha256());
    assert_eq!(
        provider.test_observation(DISPATCH_ID, changed_model).await,
        Err(ScopeAdviceProviderError::ProvenNotSent)
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn serialized_request_one_byte_over_cap_is_proven_not_sent() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = Url::parse(&format!("http://{address}/v1/systemone")).unwrap();
    let mut request = request();
    request.alternatives[0]
        .covered_obligation_ids
        .push("private-request-marker".into());
    let payload = serialize_request("jev-1.13.0", &request, &[]).unwrap();
    let provider = provider_with_caps(endpoint, Duration::from_secs(1), payload.len() - 1, 16_384);
    assert!(matches!(
        provider.prepare(&request),
        Err(ScopeAdviceProviderError::ProvenNotSent)
    ));
    let result = provider.attempt_request(DISPATCH_ID, &request).await;
    assert_eq!(result, Err(ScopeAdviceProviderError::ProvenNotSent));
    let rendered = format!("{result:?}");
    assert!(!rendered.contains("private-request-marker"));
    assert!(!rendered.contains("secret-fixture-credential"));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn http_success_sends_exact_body_once_and_does_not_leak_credential() {
    let response = serde_json::to_vec(&valid_response()).unwrap();
    let (endpoint, calls, captured, server) = fixture(FixtureResponse {
        status: 200,
        content_type: "application/json; charset=utf-8",
        body: response.clone(),
        delay: Duration::ZERO,
    })
    .await;
    let provider = provider(endpoint, Duration::from_secs(1), 16_384);
    let request = request();
    let observation = provider
        .attempt_request(DISPATCH_ID, &request)
        .await
        .unwrap();
    server.await.unwrap();
    let captured = captured.await.unwrap();
    let split = captured
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    let headers = String::from_utf8_lossy(&captured[..split]);
    assert!(
        headers.contains("authorization: Bearer secret-fixture-credential")
            || headers.contains("Authorization: Bearer secret-fixture-credential")
    );
    assert_eq!(
        &captured[split..],
        serialize_request("jev-1.13.0", &request, &[]).unwrap()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        observation.outcome,
        AdvisoryDispatchOutcome::ProviderResponse
    );
    assert_eq!(observation.send_certainty, AdvisorySendCertainty::Sent);
    assert_eq!(observation.response_payload, Some(response));
    assert!(observation.latency_ms.is_some_and(|value| value >= 0));
    assert_eq!(observation.failure_reason, None);
    let rendered = format!("{observation:?}");
    assert!(!rendered.contains("secret-fixture-credential"));
    assert!(
        observation
            .raw_response_ref
            .as_deref()
            .unwrap()
            .contains("sha256:")
    );
}

async fn assert_received_failure(
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
    maximum: usize,
    reason: ScopeAdviceProviderFailureReason,
) -> ScopeAdviceProviderObservation {
    let (endpoint, calls, _, server) = fixture(FixtureResponse {
        status,
        content_type,
        body,
        delay: Duration::ZERO,
    })
    .await;
    let observation = provider(endpoint, Duration::from_secs(1), maximum)
        .attempt_request(DISPATCH_ID, &request())
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        observation.outcome,
        AdvisoryDispatchOutcome::ProviderFailure
    );
    assert_eq!(observation.send_certainty, AdvisorySendCertainty::Sent);
    assert!(observation.answers.is_none());
    assert_eq!(observation.failure_reason, Some(reason));
    assert!(observation.latency_ms.is_some_and(|value| value >= 0));
    observation
}

#[tokio::test]
async fn malformed_oversize_non_json_and_status_are_sent_failures_without_retry() {
    let oversize = assert_received_failure(
        200,
        "application/json",
        vec![b'x'; 1025],
        1024,
        ScopeAdviceProviderFailureReason::ResponseOversize,
    )
    .await;
    assert_eq!(oversize.response_payload.as_ref().unwrap().len(), 1024);
    assert!(
        oversize
            .raw_response_ref
            .as_deref()
            .unwrap()
            .contains("oversize:observed-1025:retained-1024:sha256:")
    );
    assert_received_failure(
        200,
        "text/plain",
        b"{}".to_vec(),
        1024,
        ScopeAdviceProviderFailureReason::InvalidContentType,
    )
    .await;
    assert_received_failure(
        529,
        "application/json",
        b"{}".to_vec(),
        1024,
        ScopeAdviceProviderFailureReason::HttpStatus,
    )
    .await;
}

#[tokio::test]
async fn malformed_answers_are_retained_without_transport_interpretation() {
    for body in [
        b"{".to_vec(),
        br#"{"model":"jev-1.13.0","model":"jev-1.13.0","answers":{},"usage":null}"#.to_vec(),
    ] {
        let (endpoint, calls, _, server) = fixture(FixtureResponse {
            status: 200,
            content_type: "application/json",
            body: body.clone(),
            delay: Duration::ZERO,
        })
        .await;
        let observed = provider(endpoint, Duration::from_secs(1), 4096)
            .attempt_request(DISPATCH_ID, &request())
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(observed.response_payload, Some(body));
        assert!(observed.answers.is_none());
        assert_eq!(observed.input_tokens, None);
        assert_eq!(observed.failure_reason, None);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn body_timeout_preserves_partial_bytes_digest_count_and_typed_reason() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 20\r\n\r\nabc")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    });
    let endpoint = Url::parse(&format!("http://{address}/v1/systemone")).unwrap();
    let observation = provider(endpoint, Duration::from_millis(40), 1024)
        .attempt_request(DISPATCH_ID, &request())
        .await
        .unwrap();
    assert_eq!(observation.send_certainty, AdvisorySendCertainty::Sent);
    assert_eq!(
        observation.failure_reason,
        Some(ScopeAdviceProviderFailureReason::ResponseBodyRead)
    );
    assert_eq!(
        observation.response_payload.as_deref(),
        Some(b"abc".as_slice())
    );
    let reference = observation.raw_response_ref.as_deref().unwrap();
    assert!(reference.contains("body-read:observed-3:retained-3:sha256:"));
    assert!(observation.latency_ms.is_some_and(|value| value >= 1));
    server.abort();
}

#[tokio::test]
async fn timeout_and_connection_failure_are_sent_unknown_without_retry() {
    let (endpoint, calls, _, server) = fixture(FixtureResponse {
        status: 200,
        content_type: "application/json",
        body: serde_json::to_vec(&valid_response()).unwrap(),
        delay: Duration::from_millis(200),
    })
    .await;
    let timed_out = provider(endpoint, Duration::from_millis(20), 16_384)
        .attempt_request(DISPATCH_ID, &request())
        .await;
    assert!(
        matches!(timed_out, Err(ScopeAdviceProviderError::SentUnknown { latency_ms, .. }) if latency_ms >= 1)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    server.abort();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let closed = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = [0_u8; 1024];
        let _ = socket.read(&mut bytes).await;
    });
    let endpoint = Url::parse(&format!("http://{address}/v1/systemone")).unwrap();
    let failed = provider(endpoint, Duration::from_millis(100), 16_384)
        .attempt_request(DISPATCH_ID, &request())
        .await;
    assert!(matches!(
        failed,
        Err(ScopeAdviceProviderError::SentUnknown { .. })
    ));
    closed.await.unwrap();
}
