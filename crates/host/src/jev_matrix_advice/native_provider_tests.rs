use super::*;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
};
use tect_domain::{AdvisoryModelConfiguration, AdvisoryProviderProfileRef, MatrixRanking};
use uuid::Uuid;

fn config(endpoint: Url, maximum_response_bytes: usize) -> JevNativeMatrixConfig {
    JevNativeMatrixConfig {
        provider_identity: MatrixProviderIdentity {
            provider_profile_ref: AdvisoryProviderProfileRef {
                id: "profile".into(),
            },
            model_configuration: AdvisoryModelConfiguration {
                model: "jev-1.13.0".into(),
            },
            destination: endpoint.as_str().into(),
            wire_version: "caller-value-is-normalized".into(),
        },
        endpoint,
        timeout: Duration::from_secs(2),
        maximum_request_bytes: 262_144,
        maximum_response_bytes,
    }
}

fn loopback(response: Vec<u8>) -> (Url, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = vec![0; 4096];
        let received = stream.read(&mut request).unwrap();
        stream.write_all(&response).unwrap();
        String::from_utf8_lossy(&request[..received]).into_owned()
    });
    (endpoint, handle)
}

#[test]
fn constructor_requires_explicit_credential_and_exact_endpoint_path() {
    let endpoint = Url::parse("http://127.0.0.1:9/v1/systemone").unwrap();
    assert!(matches!(
        JevNativeMatrixProvider::new(config(endpoint.clone(), 1024), String::new()),
        Err(Error::InvalidConfiguration)
    ));
    let wrong = Url::parse("http://127.0.0.1:9/other").unwrap();
    assert!(matches!(
        JevNativeMatrixProvider::new(config(wrong, 1024), "secret".into()),
        Err(Error::InvalidConfiguration)
    ));
    let provider = JevNativeMatrixProvider::new(config(endpoint, 1024), "secret".into()).unwrap();
    assert_eq!(
        provider.identity().unwrap().wire_version,
        native_wire::NATIVE_MATRIX_WIRE_VERSION
    );
}

#[test]
fn native_response_configuration_matches_recovery_ceiling() {
    let endpoint = Url::parse("http://127.0.0.1:9/v1/systemone").unwrap();
    assert_eq!(MAX_NATIVE_MATRIX_RESPONSE_BYTES, 4 * 1024 * 1024);
    assert_eq!(super::super::MAX_MATRIX_RESPONSE_BYTES, 8 * 1024 * 1024);
    assert!(
        JevNativeMatrixProvider::new(
            config(endpoint.clone(), MAX_NATIVE_MATRIX_RESPONSE_BYTES),
            "local-test".into()
        )
        .is_ok()
    );
    assert!(matches!(
        JevNativeMatrixProvider::new(
            config(endpoint, MAX_NATIVE_MATRIX_RESPONSE_BYTES + 1),
            "local-test".into()
        ),
        Err(Error::InvalidConfiguration)
    ));
}

fn assert_context(observed: &MatrixProviderObservation, failure: Option<&str>) {
    let context = observed.original_transport_context.as_ref().unwrap();
    assert_eq!(context.send_certainty, AdvisorySendCertainty::Sent);
    assert_eq!(
        context.outcome,
        if failure.is_some() {
            AdvisoryDispatchOutcome::ProviderFailure
        } else {
            AdvisoryDispatchOutcome::ProviderResponse
        }
    );
    assert_eq!(context.provider_failure_code.as_deref(), failure);
    let bytes = observed.response_payload.as_ref().unwrap();
    assert_eq!(
        context.raw_response_ref,
        Some(format!("sha256:{:x}", Sha256::digest(bytes)))
    );
    context.validate_for(&observed.response_payload).unwrap();
    let common = tect_application::AdvisoryProviderReceiptObservation::from(observed);
    assert_eq!(common.original_transport_context.as_ref(), Some(context));
}

#[test]
fn constructor_rejects_cleartext_remote_or_dns_and_url_credentials() {
    for url in [
        "http://192.0.2.1/v1/systemone",
        "http://localhost/v1/systemone",
        "http://example.test/v1/systemone",
        "https://name:password@example.test/v1/systemone",
        "https://example.test/v1/systemone#fragment",
    ] {
        let endpoint = Url::parse(url).unwrap();
        assert!(matches!(
            JevNativeMatrixProvider::new(config(endpoint, 1024), "secret".into()),
            Err(Error::InvalidConfiguration)
        ));
    }
    for url in [
        "http://127.0.0.1/v1/systemone",
        "http://[::1]/v1/systemone",
        "https://example.test/v1/systemone",
    ] {
        let endpoint = Url::parse(url).unwrap();
        assert!(JevNativeMatrixProvider::new(config(endpoint, 1024), "secret".into()).is_ok());
    }
}

#[tokio::test]
async fn single_loopback_post_has_bearer_and_returns_bounded_bytes() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec();
    let (endpoint, server) = loopback(response);
    let provider = JevNativeMatrixProvider::new(config(endpoint, 64), "local-test".into()).unwrap();
    let observed = provider.send_once(b"{}".to_vec()).await.unwrap();
    assert_eq!(observed.response_payload, Some(b"{}".to_vec()));
    assert_eq!(observed.http_status, Some(200));
    assert!(observed.response_complete);
    assert_context(&observed, None);
    assert_eq!(observed.input_tokens, None);
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /v1/systemone HTTP/1.1"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer local-test")
    );
}

#[tokio::test]
async fn completed_error_malformed_and_empty_bodies_are_raw_observations() {
    for (status, body) in [(500, "failure"), (200, "{broken"), (204, "")] {
        let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes();
        let (endpoint, server) = loopback(response);
        let provider =
            JevNativeMatrixProvider::new(config(endpoint, 64), "local-test".into()).unwrap();
        let observed = provider.send_once(b"{}".to_vec()).await.unwrap();
        assert_eq!(observed.http_status, Some(status));
        assert!(observed.response_complete);
        assert_context(&observed, (status == 500).then_some("http-status"));
        assert_eq!(observed.response_payload, Some(body.as_bytes().to_vec()));
        assert_eq!(observed.input_tokens, None);
        assert_eq!(observed.output_tokens, None);
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("POST /v1/systemone HTTP/1.1")
        );
    }
}

#[test]
fn known_budget_header_is_exact_and_structural_not_signature_authority() {
    let header = json!({"policy_id":"fixture", "policy_version":1,"policy_digest":"a".repeat(64)});
    let snapshot = json!({"budget_policy":header});
    assert_eq!(
        validated_budget_header(&snapshot, "fixture").unwrap(),
        header
    );
    assert!(validated_budget_header(&snapshot, "different").is_err());
    for invalid in [
        json!({"policy_id":"fixture","policy_version":0,"policy_digest":"a".repeat(64)}),
        json!({"policy_id":"fixture","policy_version":1,"policy_digest":"a".repeat(64),"extra":true}),
        json!({"policy_id":"fixture","policy_version":1,"policy_digest":"bad"}),
    ] {
        assert!(validated_budget_header(&json!({"budget_policy":invalid}), "fixture").is_err());
    }
}

#[test]
fn captured_like_v2_snapshot_requires_exact_authority_and_correlation() {
    let provider = JevNativeMatrixProvider::new(
        config(
            Url::parse("https://api.typesafe.ai/v1/systemone").unwrap(),
            4096,
        ),
        "synthetic-never-sent".into(),
    )
    .unwrap();
    let identity = provider.identity().unwrap();
    let mut saved = super::super::saved_response_tests::stored_dispatch(
        b"captured-native-request".to_vec(),
        Some(br#"{"usage":{"input_tokens":3650,"output_tokens":80}}"#.to_vec()),
        json!({}),
    );
    saved.binding.verification = MatrixVerificationAuthority::ContextV2 {
        digest: "a".repeat(64),
        snapshot_id: Uuid::from_u128(4),
        authority_schema: "tect.context-matrix-requirements/1".into(),
        semantic_digest: "b".repeat(64),
    };
    saved.raw_observation_sealed = true;
    let request_hash = format!("{:x}", Sha256::digest(&saved.request_payload));
    saved.configuration_snapshot = json!({
        "provider_profile_ref": identity.provider_profile_ref,
        "model_configuration": identity.model_configuration,
        "destination": identity.destination,
        "wire_version": native_wire::NATIVE_MATRIX_WIRE_VERSION,
        "budget_policy_id": "policy",
        "request_body_length": saved.request_payload.len(),
        "request_body_sha256": request_hash,
        "budget_policy": {"policy_id":"policy","policy_version":1,"policy_digest":"c".repeat(64)},
        "matrix_authority": {
            "schema":"tect.context-matrix-verification/1",
            "verification_digest":"a".repeat(64),
            "frozen_snapshot_id":Uuid::from_u128(4),
            "authority_schema":"tect.context-matrix-requirements/1",
            "requirements_semantic_digest":"b".repeat(64),
        },
        "advisory_correlation": {
            "opportunity_id":saved.dispatch.opportunity_id,
            "dispatch_id":saved.dispatch.id,
        },
    });
    assert!(native_snapshot_matches(&identity, &saved, &request_hash).unwrap());
    assert_eq!(
        provider.sealed_response_usage(&saved).input_tokens,
        Some(3650)
    );
    assert_eq!(
        provider.sealed_response_usage(&saved).output_tokens,
        Some(80)
    );

    for pointer in [
        "/matrix_authority/verification_digest",
        "/matrix_authority/frozen_snapshot_id",
        "/matrix_authority/requirements_semantic_digest",
        "/advisory_correlation/opportunity_id",
        "/advisory_correlation/dispatch_id",
    ] {
        let mut changed = saved.clone();
        *changed.configuration_snapshot.pointer_mut(pointer).unwrap() = json!("tampered");
        assert!(
            !native_snapshot_matches(&identity, &changed, &request_hash).unwrap(),
            "{pointer}"
        );
    }
    let mut extra = saved.clone();
    extra.configuration_snapshot["unrecognized"] = json!(true);
    assert!(!native_snapshot_matches(&identity, &extra, &request_hash).unwrap());
    let mut missing = saved.clone();
    missing
        .configuration_snapshot
        .as_object_mut()
        .unwrap()
        .remove("matrix_authority");
    assert!(!native_snapshot_matches(&identity, &missing, &request_hash).unwrap());
    let mut changed_binding = saved.clone();
    changed_binding.binding.verification = MatrixVerificationAuthority::ContextV2 {
        digest: "e".repeat(64),
        snapshot_id: Uuid::from_u128(4),
        authority_schema: "tect.context-matrix-requirements/1".into(),
        semantic_digest: "b".repeat(64),
    };
    assert!(!native_snapshot_matches(&identity, &changed_binding, &request_hash).unwrap());

    let mut legacy = saved;
    legacy.binding.verification = MatrixVerificationAuthority::LegacyV1 {
        digest: "a".repeat(64),
    };
    legacy
        .configuration_snapshot
        .as_object_mut()
        .unwrap()
        .remove("matrix_authority");
    legacy
        .configuration_snapshot
        .as_object_mut()
        .unwrap()
        .remove("advisory_correlation");
    assert!(native_snapshot_matches(&identity, &legacy, &request_hash).unwrap());
}

#[test]
#[ignore = "requires explicitly supplied retained raw S02 response; never sends"]
fn retained_s02_native_response_parses_to_low_confidence_abstention() {
    let raw = std::env::var("TECT_TEST_CAPTURED_S02_RESPONSE")
        .expect("supply retained raw response from an isolated database clone");
    let prepared = native_wire::PreparedNativeMatrixRequest {
        body: Vec::new(),
        model: "jev-1.13.0".into(),
        binding: MatrixRankingBinding {
            task_id: "2dc6bcb0-98c0-4548-b531-8feb618dcc29".into(),
            task_revision: "1".into(),
            input_digest: String::new(),
            choice_set_id: "active-jev-mvp-s02-sequence".into(),
            choice_set_version: 1,
            choice_set_digest: String::new(),
            evaluation_digest: String::new(),
            verification_digest: None,
            context: None,
        },
        eligibility: MatrixAdviceEligibility::EligibleForAdvice {
            candidate_ids: vec![
                "matrix-local-evidence-first".into(),
                "matrix-trust-first".into(),
            ],
        },
        token_to_candidate_id: BTreeMap::from([
            ("C0".into(), "matrix-local-evidence-first".into()),
            ("C1".into(), "matrix-trust-first".into()),
        ]),
    };
    let parsed = native_wire::parse_native_response(raw.as_bytes(), &prepared, 4096).unwrap();
    assert!(matches!(
        (parsed.input_tokens, parsed.output_tokens),
        (3650 | 3656, 80)
    ));
    assert!(parsed.signals.candidate_scores[1].answer_confidence < 0.70);
    assert!(matches!(
        compose_native_matrix_ranking(&prepared.eligibility, &parsed.signals).unwrap(),
        MatrixRanking::Abstained { .. }
    ));
}

#[test]
fn native_usage_is_read_only_after_durable_raw_seal() {
    let provider = JevNativeMatrixProvider::new(
        config(Url::parse("http://127.0.0.1:9/v1/systemone").unwrap(), 1024),
        "local-test".into(),
    )
    .unwrap();
    let mut saved = super::super::saved_response_tests::stored_dispatch(
        vec![],
        Some(br#"{"usage":{"input_tokens":7,"output_tokens":3}}"#.to_vec()),
        json!({}),
    );
    let original = saved.clone();
    assert_eq!(provider.sealed_response_usage(&saved).input_tokens, None);
    saved.raw_observation_sealed = true;
    saved.response_complete = false;
    assert_eq!(provider.sealed_response_usage(&saved).input_tokens, None);
    saved.response_complete = true;
    let sealed = saved.clone();
    let usage = provider.sealed_response_usage(&saved);
    assert_eq!(
        (usage.input_tokens, usage.output_tokens),
        (Some(7), Some(3))
    );
    assert!(
        saved == sealed,
        "usage extraction never changes sealed observations"
    );
    assert_eq!(saved.response_payload, original.response_payload);
    saved.response_payload =
        Some(br#"{"usage":{"input_tokens":-1,"output_tokens":null}}"#.to_vec());
    let usage = provider.sealed_response_usage(&saved);
    assert_eq!((usage.input_tokens, usage.output_tokens), (None, None));
    for raw in [
        br#"{"usage":{"input_tokens":999999,"input_tokens":0,"output_tokens":30}}"#.as_slice(),
        br#"{"usage":{"input_tokens":999999},"usage":{"input_tokens":0,"output_tokens":30}}"#.as_slice(),
        br#"{"answers":{"choice":"ABSTAIN","choice":"C0"},"usage":{"input_tokens":20,"output_tokens":30}}"#.as_slice(),
    ] {
        saved.response_payload = Some(raw.to_vec());
        let original = saved.clone();
        let usage = provider.sealed_response_usage(&saved);
        assert_eq!((usage.input_tokens, usage.output_tokens), (None, None));
        assert!(saved == original);
    }
}

#[tokio::test]
async fn oversized_loopback_response_retains_bounded_prefix_and_status() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 5\r\nConnection: close\r\n\r\n12345".to_vec();
    let (endpoint, server) = loopback(response);
    let provider = JevNativeMatrixProvider::new(config(endpoint, 4), "local-test".into()).unwrap();
    let observed = provider.send_once(b"{}".to_vec()).await.unwrap();
    assert_eq!(observed.response_payload, Some(b"1234".to_vec()));
    assert_eq!(observed.http_status, Some(200));
    assert!(!observed.response_complete);
    assert_context(&observed, Some("response-oversize"));
    assert_eq!(observed.input_tokens, None);
    server.join().unwrap();
}

#[tokio::test]
async fn truncated_body_retains_prefix_and_received_status() {
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\n123".to_vec();
    let (endpoint, server) = loopback(response);
    let provider = JevNativeMatrixProvider::new(config(endpoint, 64), "local-test".into()).unwrap();
    let observed = provider.send_once(b"{}".to_vec()).await.unwrap();
    assert_eq!(observed.response_payload, Some(b"123".to_vec()));
    assert_eq!(observed.http_status, Some(200));
    assert!(!observed.response_complete);
    assert_context(&observed, Some("response-body-read"));
    assert_eq!(observed.input_tokens, None);
    server.join().unwrap();
}

#[tokio::test]
async fn exact_configured_response_boundary_is_complete() {
    let response =
        b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n1234".to_vec();
    let (endpoint, server) = loopback(response);
    let provider = JevNativeMatrixProvider::new(config(endpoint, 4), "local-test".into()).unwrap();
    let observed = provider.send_once(b"{}".to_vec()).await.unwrap();
    assert_eq!(observed.response_payload, Some(b"1234".to_vec()));
    assert!(observed.response_complete);
    assert_context(&observed, None);
    server.join().unwrap();
}
