#[test]
fn dispatch_shape_preserves_send_uncertainty() {
    let input = AdvisoryDispatchAuthorization {
        dispatch_id: Uuid::new_v4(),
        opportunity_id: Uuid::new_v4(),
        predecessor_dispatch_id: None,
        attempt_number: 1,
        retry_basis: AdvisoryRetryBasis::Initial,
        provider: "jev".into(),
        model: "default".into(),
        configuration_snapshot: serde_json::json!({"profile":"test"}),
        configuration_digest: "a".repeat(64),
        material_digest: "b".repeat(64),
        payload_digest: "c".repeat(64),
        request_payload: b"request".to_vec(),
    };
    assert_eq!(input.validate(), Ok(()));
    let unknown = AdvisoryDispatchSeal {
        dispatch_id: input.dispatch_id,
        send_certainty: AdvisorySendCertainty::SentUnknown,
        outcome: AdvisoryDispatchOutcome::ProviderFailure,
        response_payload: None,
        input_tokens: None,
        output_tokens: None,
        latency_ms: None,
        raw_response_ref: None,
    };
    assert_eq!(unknown.validate(), Ok(()));
}

#[test]
fn reconciliation_evidence_is_typed_and_fail_closed() {
    let dispatch_id = Uuid::new_v4();
    assert_eq!(
        AdvisoryReconciliationEvidence::Inconclusive { dispatch_id }.validate(),
        Ok(())
    );
    assert_eq!(
        AdvisoryReconciliationEvidence::ConfirmedNotSent {
            dispatch_id,
            evidence_ref: "fixture:transport-not-opened".into(),
        }
        .validate(),
        Ok(())
    );
    assert_eq!(
        AdvisoryReconciliationEvidence::ConfirmedSent(AdvisoryDispatchSeal {
            dispatch_id,
            send_certainty: AdvisorySendCertainty::SentUnknown,
            outcome: AdvisoryDispatchOutcome::ProviderFailure,
            response_payload: None,
            input_tokens: None,
            output_tokens: None,
            latency_ms: None,
            raw_response_ref: None,
        })
        .validate(),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn audit_filters_are_bounded_and_decision_point_compatible() {
    let valid = AdvisoryAuditQuery {
        limit: 100,
        scope_id: Some(Uuid::new_v4()),
        after: Some(Uuid::new_v4()),
        capability: Some(AdvisoryCapability::ScopeDecomposition),
        decision_point: Some(AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection),
        reason: Some(AdvisoryReason::RequestSkip),
        state: Some(AdvisoryOpportunityState::NoCall),
    };
    assert_eq!(valid.validate(), Ok(()));
    assert_eq!(
        AdvisoryAuditQuery {
            limit: 101,
            ..valid.clone()
        }
        .validate(),
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        AdvisoryAuditQuery {
            capability: Some(AdvisoryCapability::ModelRouting),
            ..valid
        }
        .validate(),
        Err(Error::InvalidArguments)
    );
}
