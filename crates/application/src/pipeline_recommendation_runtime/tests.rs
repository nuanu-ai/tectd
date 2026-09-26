use super::*;
use tect_domain::{AdvisoryBudgetReservation, AdvisoryRetryBasis};

struct FakeProvider;

#[async_trait]
impl PipelineRecommendationProvider for FakeProvider {
    fn prepare(
        &self,
        _: &PreparedPipelineRecommendation,
    ) -> Result<PreparedPipelineRecommendationAttempt> {
        Err(Error::TransportUnavailable)
    }

    fn parse_sealed_response(
        &self,
        _: &PipelineRecommendationManifest,
        _: &PreparedPipelineRecommendationAttempt,
        _: &SealedPipelineRecommendationResponse,
    ) -> Result<PipelineRecommendationRanking> {
        Ok(PipelineRecommendationRanking::Abstained)
    }

    async fn attempt_prepared(
        &self,
        prepared: PreparedPipelineRecommendationAttempt,
        permit: PipelineStartedDispatchPermit,
    ) -> Result<PipelineProviderObservation> {
        if !permit.permits(&prepared) {
            return Err(Error::InputConflict);
        }
        Ok(PipelineProviderObservation {
            raw_response: b"fake response".to_vec(),
            input_tokens: Some(1),
            output_tokens: Some(2),
        })
    }
}

fn identity() -> PipelineProviderIdentity {
    PipelineProviderIdentity {
        provider: "fake-jev".into(),
        model: "jev-test".into(),
        destination: "https://example.invalid/v1/systemone".into(),
        wire_version: "tect.pipeline-typesafe-native/1".into(),
    }
}

fn attempt() -> PreparedPipelineRecommendationAttempt {
    let body = b"exact request".to_vec();
    PreparedPipelineRecommendationAttempt {
        opportunity_id: Uuid::new_v4(),
        manifest_digest: "a".repeat(64),
        identity: identity(),
        body_sha256: format!("{:x}", Sha256::digest(&body)),
        body,
    }
}

fn dispatch(attempt: &PreparedPipelineRecommendationAttempt) -> AdvisoryDispatch {
    AdvisoryDispatch {
        id: Uuid::new_v4(),
        opportunity_id: attempt.opportunity_id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        provider: attempt.identity.provider.clone(),
        model: attempt.identity.model.clone(),
        configuration_digest: "b".repeat(64),
        material_digest: attempt.manifest_digest.clone(),
        payload_digest: attempt.body_sha256.clone(),
        input_tokens: None,
        output_tokens: None,
        latency_ms: None,
        state: AdvisoryDispatchState::Authorized,
        send_certainty: AdvisorySendCertainty::NotSent,
        outcome: None,
        retry_basis: AdvisoryRetryBasis::Initial,
        raw_response_ref: None,
    }
}

#[tokio::test]
async fn prepared_or_authorized_row_cannot_enter_provider_or_parser() {
    let attempt = attempt();
    let authorized = dispatch(&attempt);
    let config = serde_json::json!({
        "destination": attempt.identity.destination,
        "wire_version": attempt.identity.wire_version,
        "request_body_sha256": attempt.body_sha256,
    });
    let authorization = AdvisoryDispatchAuthorization {
        dispatch_id: authorized.id,
        opportunity_id: attempt.opportunity_id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        retry_basis: AdvisoryRetryBasis::Initial,
        provider: attempt.identity.provider.clone(),
        model: attempt.identity.model.clone(),
        configuration_digest: format!("{:x}", Sha256::digest(serde_json::to_vec(&config).unwrap())),
        configuration_snapshot: config,
        material_digest: attempt.manifest_digest.clone(),
        payload_digest: attempt.body_sha256.clone(),
        request_payload: attempt.body.clone(),
    };
    let start = AdvisoryDispatchStart {
        dispatch: authorized.clone(),
        should_send: true,
        budget_reservation: None,
    };
    assert!(
        PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
            .is_err()
    );
    assert!(
        SealedPipelineRecommendationResponse::from_saved(
            &authorized,
            &attempt,
            &attempt.body,
            b"response".to_vec(),
            &format!("{:x}", Sha256::digest(b"response")),
        )
        .is_err()
    );

    let mut sending = authorized;
    sending.state = AdvisoryDispatchState::Sending;
    sending.send_certainty = AdvisorySendCertainty::SentUnknown;
    sending.configuration_digest = authorization.configuration_digest.clone();
    let mut start = AdvisoryDispatchStart {
        dispatch: sending.clone(),
        should_send: true,
        budget_reservation: None,
    };
    assert!(matches!(
        PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt),
        Err(Error::BudgetPolicyInvalid)
    ));
    start.budget_reservation = Some(AdvisoryBudgetReservation {
        dispatch_id: sending.id,
        policy_id: Uuid::new_v4(),
        policy_version: 1,
        policy_digest: "c".repeat(64),
        policy_effective_from_unix_ms: 1,
        policy_effective_until_unix_ms: 2,
        request_sha256: attempt.body_sha256.clone(),
        request_utf8_bytes: attempt.body.len() as i64,
        reserved_calls: 1,
        reserved_retry_dispatches: 0,
        remaining_elapsed_ms: 1,
        reserved_input_tokens: 1,
        reserved_output_tokens: 1,
    });
    let wrong_digest = start.budget_reservation.as_mut().unwrap();
    wrong_digest.request_sha256 = "d".repeat(64);
    assert!(
        PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
            .is_err()
    );
    start.budget_reservation.as_mut().unwrap().request_sha256 = attempt.body_sha256.clone();
    let permit =
        PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
            .unwrap();
    let observed = FakeProvider.attempt_prepared(self::attempt(), permit).await;
    assert!(matches!(observed, Err(Error::InputConflict)));

    let permit =
        PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
            .unwrap();
    assert_eq!(permit.dispatch_id(), sending.id);
    let observed = FakeProvider
        .observe_prepared(attempt, permit)
        .await
        .unwrap();
    assert_eq!(
        observed.response_payload.as_deref(),
        Some(b"fake response".as_slice())
    );
    assert_eq!(observed.input_tokens, Some(1));
    assert_eq!(observed.output_tokens, Some(2));
    assert!(observed.response_complete);
    let transport = observed.original_transport_context.unwrap();
    assert_eq!(transport.send_certainty, AdvisorySendCertainty::Sent);
    assert_eq!(transport.outcome, AdvisoryDispatchOutcome::ProviderResponse);

    let attempt = PreparedPipelineRecommendationAttempt {
        opportunity_id: sending.opportunity_id,
        manifest_digest: sending.material_digest.clone(),
        identity: identity(),
        body_sha256: sending.payload_digest.clone(),
        body: b"exact request".to_vec(),
    };

    sending.state = AdvisoryDispatchState::Sealed;
    sending.send_certainty = AdvisorySendCertainty::Sent;
    sending.outcome = Some(AdvisoryDispatchOutcome::ProviderResponse);
    assert!(
        SealedPipelineRecommendationResponse::from_saved(
            &sending,
            &attempt,
            b"wrong request",
            b"response".to_vec(),
            &format!("{:x}", Sha256::digest(b"response")),
        )
        .is_err()
    );
    assert!(
        SealedPipelineRecommendationResponse::from_saved(
            &sending,
            &attempt,
            &attempt.body,
            b"response".to_vec(),
            "wrong digest",
        )
        .is_err()
    );
    assert!(
        SealedPipelineRecommendationResponse::from_saved(
            &sending,
            &attempt,
            &attempt.body,
            b"response".to_vec(),
            &format!("{:x}", Sha256::digest(b"response")),
        )
        .is_ok()
    );
}
