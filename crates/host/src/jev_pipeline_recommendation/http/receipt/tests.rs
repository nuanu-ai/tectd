use super::*;
use crate::jev_pipeline_recommendation::tests::{http_fixture, http_provider, prepared, response};
use tect_domain::{
    AdvisoryDispatch, AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryReason,
    AdvisoryRequestPreference, AdvisoryRetryBasis,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const DISPATCH: Uuid = Uuid::from_u128(15);

#[tokio::test]
async fn complete_raw_receipts_preserve_status_and_never_decode_usage() {
    for (status, bytes, failure) in [
        (
            200,
            br#"{"usage":{"input_tokens":20,"output_tokens":30}}"#.to_vec(),
            None,
        ),
        (200, b"{invalid".to_vec(), None),
        (500, b"provider failure".to_vec(), Some("http-status")),
        (200, vec![], Some("empty-response")),
    ] {
        let (endpoint, captured, server) =
            http_fixture(status, bytes.clone(), Duration::ZERO).await;
        let provider = http_provider(endpoint, Duration::from_secs(2), MAX_RESPONSE_BYTES);
        let observed = provider
            .observe_once(DISPATCH, b"frozen-request".to_vec())
            .await
            .unwrap();
        captured.await.unwrap();
        server.await.unwrap();
        assert_eq!(observed.response_payload, Some(bytes.clone()));
        assert_eq!(observed.http_status, Some(status));
        assert!(observed.response_complete);
        assert_eq!(
            (observed.input_tokens, observed.output_tokens),
            (None, None)
        );
        let context = observed.original_transport_context.unwrap();
        assert_eq!(context.send_certainty, AdvisorySendCertainty::Sent);
        assert_eq!(context.provider_failure_code.as_deref(), failure);
        assert_eq!(
            context.outcome,
            if failure.is_some() {
                AdvisoryDispatchOutcome::ProviderFailure
            } else {
                AdvisoryDispatchOutcome::ProviderResponse
            }
        );
        assert!(
            context
                .raw_response_ref
                .unwrap()
                .contains(&format!("{DISPATCH}:sha256:{:x}", Sha256::digest(bytes)))
        );
    }
}

async fn custom_fixture(
    content_type: &str,
    declared_length: usize,
    bytes: Vec<u8>,
) -> (JevPipelineProvider, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {declared_length}\r\nConnection: close\r\n\r\n"
    );
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0_u8; 4096];
        let received = socket.read(&mut request).await.unwrap();
        assert!(received > 0);
        socket.write_all(header.as_bytes()).await.unwrap();
        socket.write_all(&bytes).await.unwrap();
    });
    (
        http_provider(endpoint, Duration::from_secs(2), MAX_RESPONSE_BYTES),
        task,
    )
}

#[tokio::test]
async fn content_type_partial_and_oversize_keep_received_bytes() {
    let prefix = br#"{"usage":{"input_tokens":20,"output_tokens":30}}"#.to_vec();
    for (content_type, declared, bytes, complete, code, expected) in [
        (
            "text/plain",
            prefix.len(),
            prefix.clone(),
            true,
            "content-type",
            prefix.clone(),
        ),
        (
            "application/json",
            prefix.len() + 20,
            prefix.clone(),
            false,
            "response-body-read",
            prefix,
        ),
        (
            "application/json",
            MAX_RESPONSE_BYTES + 1,
            vec![b'x'; MAX_RESPONSE_BYTES + 1],
            false,
            "response-oversize",
            vec![b'x'; MAX_RESPONSE_BYTES],
        ),
    ] {
        let (provider, server) = custom_fixture(content_type, declared, bytes).await;
        let observed = provider
            .observe_once(DISPATCH, b"{}".to_vec())
            .await
            .unwrap();
        server.await.unwrap();
        assert_eq!(observed.response_payload, Some(expected));
        assert_eq!(observed.http_status, Some(200));
        assert_eq!(observed.response_complete, complete);
        assert_eq!(
            (observed.input_tokens, observed.output_tokens),
            (None, None)
        );
        let context = observed.original_transport_context.unwrap();
        assert_eq!(context.send_certainty, AdvisorySendCertainty::Sent);
        assert_eq!(context.outcome, AdvisoryDispatchOutcome::ProviderFailure);
        assert_eq!(context.provider_failure_code.as_deref(), Some(code));
    }
}

#[tokio::test]
async fn absent_response_stays_sent_unknown() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    drop(listener);
    let observed = http_provider(endpoint, Duration::from_secs(1), MAX_RESPONSE_BYTES)
        .observe_once(DISPATCH, b"{}".to_vec())
        .await
        .unwrap();
    assert_eq!(observed.response_payload, None);
    assert_eq!(observed.http_status, None);
    assert!(!observed.response_complete);
    assert_eq!(
        observed.original_transport_context.unwrap().send_certainty,
        AdvisorySendCertainty::SentUnknown
    );
}

fn fixture() -> (JevPipelineProvider, StoredAdvisoryProviderReceipt) {
    let provider = http_provider(
        Url::parse("http://127.0.0.1:9/v1/systemone").unwrap(),
        Duration::from_secs(1),
        MAX_RESPONSE_BYTES,
    );
    let native = prepared(2);
    let snapshot = serde_json::json!({
        "provider_profile_ref": provider.config.identity.provider,
        "model_configuration": {"model": provider.config.identity.model},
        "destination": provider.config.identity.destination,
        "wire_version": WIRE_VERSION,
        "request_body_sha256": format!("{:x}", Sha256::digest(&native.body)),
    });
    let opportunity = AdvisoryOpportunity {
        id: Uuid::from_u128(1),
        workspace_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        authorized_actor_id: Uuid::from_u128(4),
        capability: AdvisoryCapability::PipelineRecommendation,
        decision_point: AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen,
        decision_point_version: 1,
        workflow_occurrence_key: "fixture".into(),
        target_kind: "slice_candidate_node".into(),
        target_id: Some(Uuid::from_u128(1)),
        work_revision: Some(1),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        matrix_verification_digest: None,
        source_ref: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        config_revision: 1,
        material_digest: native.manifest_digest.clone(),
        state: AdvisoryOpportunityState::AwaitingResponse,
        primary_reason: AdvisoryReason::DispatchAuthorized,
        provider_called: true,
    };
    let dispatch = AdvisoryDispatch {
        id: DISPATCH,
        opportunity_id: opportunity.id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        provider: provider.config.identity.provider.clone(),
        model: provider.config.identity.model.clone(),
        material_digest: native.manifest_digest,
        configuration_digest: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&snapshot).unwrap())
        ),
        payload_digest: format!("{:x}", Sha256::digest(&native.body)),
        input_tokens: None,
        output_tokens: None,
        latency_ms: Some(7),
        state: AdvisoryDispatchState::Sending,
        send_certainty: AdvisorySendCertainty::SentUnknown,
        outcome: None,
        retry_basis: AdvisoryRetryBasis::Initial,
        raw_response_ref: None,
    };
    let raw = serde_json::to_vec(&response(&prepared(2))).unwrap();
    let observed = observation(&provider, DISPATCH, Some(raw), Some(200), true, None);
    let request_hash = snapshot["request_body_sha256"].as_str().unwrap().to_owned();
    (
        provider,
        StoredAdvisoryProviderReceipt {
            opportunity,
            dispatch,
            configuration_snapshot: snapshot,
            request_payload: native.body,
            request_payload_sha256: request_hash,
            observation: Some(observed),
            original_elapsed_ms: Some(7),
        },
    )
}

#[test]
fn sealed_usage_recovery_is_pure_complete_only_and_duplicate_safe() {
    let (provider, mut saved) = fixture();
    let original = saved.observation.clone();
    let usage = provider.usage_from_sealed_response(&saved).unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens),
        (Some(20), Some(30))
    );
    assert_eq!(saved.observation, original);
    for raw in [
        br#"{"usage":{"input_tokens":999999,"input_tokens":0,"output_tokens":30}}"#.as_slice(),
        br#"{"usage":{"input_tokens":999999},"usage":{"input_tokens":0,"output_tokens":30}}"#
            .as_slice(),
        b"{broken",
        b"",
        b"{}",
    ] {
        saved.observation.as_mut().unwrap().response_payload = Some(raw.to_vec());
        assert_eq!(
            provider.usage_from_sealed_response(&saved).unwrap(),
            AdvisoryProviderReceiptUsage::default()
        );
    }
    saved.observation = original;
    saved.observation.as_mut().unwrap().response_complete = false;
    assert_eq!(
        provider.usage_from_sealed_response(&saved).unwrap(),
        AdvisoryProviderReceiptUsage::default()
    );
    saved.observation = None;
    assert_eq!(
        provider.usage_from_sealed_response(&saved).unwrap(),
        AdvisoryProviderReceiptUsage::default()
    );
}

#[test]
fn frozen_reconstruction_rejects_changed_full_manifest_or_metadata() {
    let (provider, mut saved) = fixture();
    assert!(
        validate_body(
            &provider,
            &saved.request_payload,
            &saved.opportunity.material_digest
        )
        .is_ok()
    );
    let original_body = saved.request_payload.clone();
    let mut body = crate::jev_json::decode_unique_json(&original_body).unwrap();
    body["state"]["manifest"]["mandatory_card_ids"] = serde_json::json!([]);
    let changed = serde_json::to_vec(&body).unwrap();
    assert!(validate_body(&provider, &changed, &saved.opportunity.material_digest).is_err());
    body = crate::jev_json::decode_unique_json(&original_body).unwrap();
    body["questions"] = serde_json::json!({});
    assert!(
        validate_body(
            &provider,
            &serde_json::to_vec(&body).unwrap(),
            &saved.opportunity.material_digest
        )
        .is_err()
    );
    saved.configuration_snapshot["destination"] =
        serde_json::json!("http://127.0.0.1:10/v1/systemone");
    assert_eq!(
        provider.usage_from_sealed_response(&saved),
        Err(Error::InputConflict)
    );
}
