use super::super::wire::serialize_request;
use super::helpers::{native_request as request, native_response};
use super::*;

fn valid_response() -> Value {
    native_response(&request(), false)
}

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tect_application::ScopeAdviceProviderError;
use tect_domain::{AdvisoryDispatchOutcome, AdvisorySendCertainty};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;

mod fixture;
use fixture::{FixtureResponse, fixture, provider, provider_with_caps};

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
    assert_eq!(prepared.wire_version(), "jev-system-one-json/3");
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
    let context = helpers::native_context();
    let request = context.request();
    let response = serde_json::to_vec(&native_response(request, false)).unwrap();
    let (endpoint, calls, captured, server) = fixture(FixtureResponse {
        status: 200,
        content_type: "application/json; charset=utf-8",
        body: response.clone(),
        delay: Duration::ZERO,
    })
    .await;
    let provider = provider(endpoint, Duration::from_secs(1), 16384);
    let prepared = provider.prepare_context(&context).unwrap();
    let expected_body = prepared.body().to_vec();
    // Saved is isolated unit data; no durable store operation occurs here.
    let mut saved = helpers::synthetic_saved(&prepared, helpers::raw_response(response.clone()));
    let started = std::time::Instant::now();
    let raw = provider
        .observe_transport(DISPATCH_ID, prepared)
        .await
        .unwrap();
    assert!(elapsed_ms(started) >= 0);
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
    assert_eq!(&captured[split..], expected_body);
    assert_eq!(
        expected_body,
        serialize_request("jev-1.13.0", request, context.emitted()).unwrap()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(raw.legacy_answers, None);
    assert_eq!(raw.receipt.response_payload, Some(response));
    assert_eq!(
        (raw.receipt.input_tokens, raw.receipt.output_tokens),
        (None, None)
    );
    assert_eq!(raw.receipt.http_status, Some(200));
    assert!(raw.receipt.response_complete);
    let transport = raw.receipt.original_transport_context.as_ref().unwrap();
    assert_eq!(transport.outcome, AdvisoryDispatchOutcome::ProviderResponse);
    assert_eq!(transport.send_certainty, AdvisorySendCertainty::Sent);
    assert_eq!(transport.provider_failure_code, None);
    assert!(
        transport
            .raw_response_ref
            .as_deref()
            .unwrap()
            .contains("sha256:")
    );
    assert!(!format!("{:?}", raw.receipt).contains("secret-fixture-credential"));
    saved.observation = Some(raw.receipt);
    let before = saved.observation.clone();
    let usage = super::super::sealed::usage(&provider, &saved).unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens),
        (Some(11), Some(5))
    );
    let prepared = provider.prepare_context(&context).unwrap();
    let answers = super::super::sealed::parse(&provider, &prepared, &saved).unwrap();
    assert_eq!(answers.answers.len(), request.alternatives.len());
    assert!(matches!(
        answers.comparative_disposition,
        Some(tect_domain::ComparativeDisposition::Selected(_))
    ));
    assert_eq!(saved.observation, before);
}

#[tokio::test]
async fn malformed_oversize_non_json_and_status_are_sent_failures_without_retry() {
    assert_uninterpreted_response(b"{".to_vec(), 1024).await;
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
async fn duplicate_keys_at_nested_levels_are_rejected_after_one_http_call() {
    let choice = format!("choice_{ID}");
    for body in [
        br#"{"model":"jev-1.13.0","model":"jev-1.13.0","answers":{},"usage":null}"#.to_vec(),
        format!(r#"{{"model":"jev-1.13.0","answers":{{"{choice}":{{"type":"choice","type":"choice"}}}},"usage":null}}"#).into_bytes(),
        format!(r#"{{"model":"jev-1.13.0","answers":{{"{choice}":{{"type":"choice","choice":"PREFERRED","confidence":0.8,"probabilities":{{"PREFERRED":0.8,"PREFERRED":0.8,"NON_PREFERRED":0.2}}}}}},"usage":null}}"#).into_bytes(),
    ] {
        assert_uninterpreted_response(body,4096).await;
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
    let provider = provider(endpoint, Duration::from_millis(40), 1024);
    let context = helpers::native_context();
    let prepared = provider.prepare_context(&context).unwrap();
    let started = std::time::Instant::now();
    let raw = provider
        .observe_transport(DISPATCH_ID, prepared)
        .await
        .unwrap();
    assert_eq!(raw.receipt.http_status, Some(200));
    assert!(!raw.receipt.response_complete);
    assert_eq!(raw.legacy_answers, None);
    assert_eq!(
        (raw.receipt.input_tokens, raw.receipt.output_tokens),
        (None, None)
    );
    let transport = raw.receipt.original_transport_context.as_ref().unwrap();
    assert_eq!(transport.outcome, AdvisoryDispatchOutcome::ProviderFailure);
    let observation = ScopeAdviceProviderObservation {
        send_certainty: transport.send_certainty,
        outcome: transport.outcome,
        answers: None,
        response_payload: raw.receipt.response_payload.clone(),
        input_tokens: None,
        output_tokens: None,
        latency_ms: Some(elapsed_ms(started)),
        raw_response_ref: transport.raw_response_ref.clone(),
        failure_reason: transport
            .provider_failure_code
            .as_deref()
            .and_then(ScopeAdviceProviderFailureReason::from_code),
    };
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
    let provider = provider(endpoint, Duration::from_secs(1), maximum);
    let context = helpers::native_context();
    let prepared = provider.prepare_context(&context).unwrap();
    let mut saved = helpers::synthetic_saved(&prepared, helpers::raw_response(Vec::new()));
    let started = std::time::Instant::now();
    let raw = provider
        .observe_transport(DISPATCH_ID, prepared)
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(raw.legacy_answers, None);
    assert_eq!(
        (raw.receipt.input_tokens, raw.receipt.output_tokens),
        (None, None)
    );
    assert_eq!(raw.receipt.http_status, Some(status));
    assert_eq!(
        raw.receipt.response_complete,
        !matches!(
            reason,
            ScopeAdviceProviderFailureReason::ResponseOversize
                | ScopeAdviceProviderFailureReason::ResponseBodyRead
        )
    );
    let transport = raw.receipt.original_transport_context.as_ref().unwrap();
    assert_eq!(transport.outcome, AdvisoryDispatchOutcome::ProviderFailure);
    assert_eq!(transport.send_certainty, AdvisorySendCertainty::Sent);
    assert_eq!(
        transport.provider_failure_code.as_deref(),
        Some(reason.as_code())
    );
    let observation = ScopeAdviceProviderObservation {
        send_certainty: transport.send_certainty,
        outcome: transport.outcome,
        answers: None,
        response_payload: raw.receipt.response_payload.clone(),
        input_tokens: None,
        output_tokens: None,
        latency_ms: Some(elapsed_ms(started)),
        raw_response_ref: transport.raw_response_ref.clone(),
        failure_reason: Some(reason),
    };
    assert!(observation.latency_ms.is_some_and(|value| value >= 0));
    saved.observation = Some(raw.receipt);
    let prepared = provider.prepare_context(&context).unwrap();
    assert!(super::super::sealed::parse(&provider, &prepared, &saved).is_err());
    observation
}

async fn assert_uninterpreted_response(body: Vec<u8>, maximum: usize) {
    let (endpoint, calls, _, server) = fixture(FixtureResponse {
        status: 200,
        content_type: "application/json",
        body: body.clone(),
        delay: Duration::ZERO,
    })
    .await;
    let provider = provider(endpoint, Duration::from_secs(1), maximum);
    let context = helpers::native_context();
    let prepared = provider.prepare_context(&context).unwrap();
    let mut saved = helpers::synthetic_saved(&prepared, helpers::raw_response(body.clone()));
    let raw = provider
        .observe_transport(DISPATCH_ID, prepared)
        .await
        .unwrap();
    server.await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(raw.legacy_answers, None);
    assert_eq!(
        (raw.receipt.input_tokens, raw.receipt.output_tokens),
        (None, None)
    );
    assert_eq!(raw.receipt.http_status, Some(200));
    assert!(raw.receipt.response_complete);
    assert_eq!(raw.receipt.response_payload, Some(body));
    let transport = raw.receipt.original_transport_context.as_ref().unwrap();
    assert_eq!(transport.outcome, AdvisoryDispatchOutcome::ProviderResponse);
    assert_eq!(transport.send_certainty, AdvisorySendCertainty::Sent);
    assert_eq!(transport.provider_failure_code, None);
    saved.observation = Some(raw.receipt);
    let before = saved.observation.clone();
    let prepared = provider.prepare_context(&context).unwrap();
    assert!(super::super::sealed::parse(&provider, &prepared, &saved).is_err());
    assert_eq!(saved.observation, before);
}
