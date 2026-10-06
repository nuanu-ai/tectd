use super::*;
use tect_domain::{AdvisoryBudgetReservation, AdvisoryRetryBasis};

use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct FakeProvider {
    accepted_attempts: AtomicUsize,
}

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
        self.accepted_attempts.fetch_add(1, Ordering::SeqCst);
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
    let provider = FakeProvider::default();
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
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let mut opportunity = tect_domain::AdvisoryOpportunity {
        id: attempt.opportunity_id,
        workspace_id: workspace,
        session_id: Uuid::new_v4(),
        authorized_actor_id: Uuid::new_v4(),
        capability: AdvisoryCapability::PipelineRecommendation,
        decision_point: tect_domain::AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen,
        decision_point_version: 1,
        workflow_occurrence_key: "pipeline-fixture".into(),
        target_kind: "slice_candidate_node".into(),
        target_id: Some(Uuid::new_v4()),
        work_revision: Some(1),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        matrix_verification_digest: None,
        source_ref: None,
        session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        config_revision: 1,
        material_digest: attempt.manifest_digest.clone(),
        state: AdvisoryOpportunityState::AwaitingResponse,
        primary_reason: tect_domain::AdvisoryReason::DispatchAuthorized,
        provider_called: true,
    };
    let continuation = crate::AdvisoryDispatchContinuation::after_committed_start(
        tenant,
        workspace,
        &opportunity,
        &start,
        &authorization,
    )
    .unwrap();
    assert_eq!(continuation.tenant_id(), tenant);
    assert_eq!(continuation.workspace_id(), workspace);
    assert_eq!(continuation.actor_id(), opportunity.authorized_actor_id);
    assert_eq!(continuation.opportunity_id(), opportunity.id);
    assert_eq!(continuation.dispatch_id(), sending.id);
    assert_eq!(continuation.capability(), opportunity.capability);
    assert_eq!(continuation.decision_point(), opportunity.decision_point);
    assert_eq!(continuation.target_kind(), opportunity.target_kind);
    assert_eq!(continuation.target_id(), opportunity.target_id);
    assert_eq!(continuation.work_revision(), opportunity.work_revision);
    assert_eq!(continuation.material_digest(), sending.material_digest);
    assert_eq!(
        continuation.configuration_digest(),
        sending.configuration_digest
    );
    assert_eq!(continuation.request_sha256(), sending.payload_digest);
    let recovered =
        crate::AdvisoryDispatchContinuation::from_saved(tenant, workspace, &opportunity, &sending)
            .unwrap();
    assert_eq!(recovered.dispatch_id(), continuation.dispatch_id());
    assert_eq!(recovered.target_id(), continuation.target_id());
    assert_eq!(recovered.request_sha256(), continuation.request_sha256());
    assert_eq!(sending.state, AdvisoryDispatchState::Sending);
    assert_eq!(sending.send_certainty, AdvisorySendCertainty::SentUnknown);
    assert_eq!(provider.accepted_attempts.load(Ordering::SeqCst), 0);

    opportunity.capability = AdvisoryCapability::ScopeDecomposition;
    assert!(matches!(
        crate::AdvisoryDispatchContinuation::after_committed_start(
            tenant,
            workspace,
            &opportunity,
            &start,
            &authorization,
        ),
        Err(Error::InputConflict)
    ));
    opportunity.capability = AdvisoryCapability::PipelineRecommendation;
    opportunity.decision_point =
        tect_domain::AdvisoryDecisionPoint::EngineeringProfileBeforeSelection;
    assert!(matches!(
        crate::AdvisoryDispatchContinuation::after_committed_start(
            tenant,
            workspace,
            &opportunity,
            &start,
            &authorization,
        ),
        Err(Error::InputConflict)
    ));
    opportunity.decision_point =
        tect_domain::AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen;
    opportunity.target_kind = "matrix_task".into();
    assert!(matches!(
        crate::AdvisoryDispatchContinuation::after_committed_start(
            tenant,
            workspace,
            &opportunity,
            &start,
            &authorization,
        ),
        Err(Error::InputConflict)
    ));
    opportunity.target_kind = "slice_candidate_node".into();
    let target = opportunity.target_id.take();
    assert!(matches!(
        crate::AdvisoryDispatchContinuation::after_committed_start(
            tenant,
            workspace,
            &opportunity,
            &start,
            &authorization,
        ),
        Err(Error::InputConflict)
    ));
    opportunity.target_id = target;
    assert_eq!(provider.accepted_attempts.load(Ordering::SeqCst), 0);

    let permit =
        PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
            .unwrap();
    let observed = provider.attempt_prepared(self::attempt(), permit).await;
    assert!(matches!(observed, Err(Error::InputConflict)));
    assert_eq!(provider.accepted_attempts.load(Ordering::SeqCst), 0);

    let permit =
        PipelineStartedDispatchPermit::after_committed_start(&start, &authorization, &attempt)
            .unwrap();
    assert_eq!(permit.dispatch_id(), sending.id);
    let observed = provider.observe_prepared(attempt, permit).await.unwrap();
    assert_eq!(
        observed.response_payload.as_deref(),
        Some(b"fake response".as_slice())
    );
    assert_eq!(observed.input_tokens, Some(1));
    assert_eq!(observed.output_tokens, Some(2));
    assert!(observed.response_complete);
    assert_eq!(provider.accepted_attempts.load(Ordering::SeqCst), 1);
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
