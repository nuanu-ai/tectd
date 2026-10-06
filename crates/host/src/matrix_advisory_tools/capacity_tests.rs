use super::*;
use std::cell::Cell;
use tect_domain::{AdvisoryCapability, AdvisoryDecisionPoint, advisory_reason_matches_state};

// One list drives enumeration and a wildcard-free match. A new Domain variant
// makes this test module fail compilation until its encoding is covered.
macro_rules! variants {
    ($ty:ident, $name:ident, [$($variant:ident),+ $(,)?]) => {
        fn $name() -> &'static [$ty] {
            fn exhaustive(value: $ty) {
                match value { $($ty::$variant => (),)+ }
            }
            const VALUES: &[$ty] = &[$($ty::$variant),+];
            for &value in VALUES { exhaustive(value); }
            VALUES
        }
    };
}

variants!(
    AdvisoryOpportunityState,
    states,
    [
        Prepared,
        NoCall,
        AwaitingResponse,
        Advised,
        Invalidated,
        Failed,
        Unresolved
    ]
);
variants!(
    AdvisoryReason,
    reasons,
    [
        WorkspaceDisabled,
        SessionSkip,
        RequestSkip,
        ChoiceSetNotApplicable,
        MatrixEvidenceUnresolved,
        MatrixSourceUnverified,
        MatrixTaskUnbound,
        MatrixSnapshotMissing,
        MatrixBindingMismatch,
        MatrixContextUnresolved,
        MatrixContextStale,
        MatrixAuthoritySchemaUnsupported,
        MatrixOperatingEvidenceUnresolved,
        DeterministicInputInvalid,
        CapabilityUnavailable,
        ProviderUnconfigured,
        BudgetPolicyInvalid,
        BudgetExhaustedAfterResponse,
        ConfigurationChanged,
        MatrixTaskRevisionChanged,
        MatrixVerificationStale,
        RecommendationPrepared,
        DispatchAuthorized,
        ProviderResponse,
        ProviderFailure,
        SendUnknown
    ]
);

fn request(key: &str) -> RequestEngineeringAdvisory {
    RequestEngineeringAdvisory {
        task_id: Uuid::from_u128(1),
        expected_task_revision: 1,
        request_key: key.into(),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    }
}

fn opportunity(request: &RequestEngineeringAdvisory) -> AdvisoryOpportunity {
    AdvisoryOpportunity {
        id: Uuid::from_u128(2),
        workspace_id: Uuid::from_u128(3),
        session_id: Uuid::from_u128(4),
        authorized_actor_id: Uuid::from_u128(5),
        capability: AdvisoryCapability::EngineeringProfile,
        decision_point: AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
        decision_point_version: 1,
        workflow_occurrence_key: request.request_key.clone(),
        target_kind: "matrix_task".into(),
        target_id: Some(request.task_id),
        work_revision: Some(1),
        matrix_task_revision: Some(1),
        matrix_choice_set_digest: Some("f".repeat(64)),
        matrix_verification_digest: Some("e".repeat(64)),
        source_ref: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        config_revision: 0,
        material_digest: "f".repeat(64),
        state: AdvisoryOpportunityState::NoCall,
        primary_reason: AdvisoryReason::MatrixOperatingEvidenceUnresolved,
        provider_called: false,
    }
}

fn keys() -> Vec<String> {
    vec![
        "key".into(),
        "\"".repeat(256),
        "\\".repeat(256),
        "a\n\t\u{0001}z".into(),
        format!("a{}z", "\u{0001}\n\t".repeat(84)),
        "é".repeat(128),
        "🦀".repeat(64),
    ]
}

fn encoded_receipt(value: AdvisoryOpportunity) -> usize {
    crate::responses::encoded_len(&crate::responses::with_actions(
        receipt(value),
        Vec::new(),
        None,
    ))
    .unwrap()
}

#[test]
fn projection_bounds_every_current_valid_state_reason_and_receipt_shape() {
    for key in keys() {
        assert!(valid_request_key(&key));
        let request = request(&key);
        let bound = crate::responses::encoded_len(&request_projection(&request)).unwrap();
        let mut valid_pairs = 0;
        for &state in states() {
            for &reason in reasons() {
                if !advisory_reason_matches_state(state, reason) {
                    continue;
                }
                valid_pairs += 1;
                for revision in [i64::MIN, -1, 0, 1, i64::MAX] {
                    // The receipt encoder accepts typed stored records; the oracle
                    // also covers defensive integer extremes, not authority checks.
                    for populated in [false, true] {
                        for provider_called in [false, true] {
                            let mut value = opportunity(&request);
                            value.state = state;
                            value.primary_reason = reason;
                            value.config_revision = revision;
                            value.matrix_task_revision = populated.then_some(revision);
                            value.target_id = populated.then_some(request.task_id);
                            value.matrix_choice_set_digest = populated.then(|| "f".repeat(64));
                            value.provider_called = provider_called;
                            assert!(encoded_receipt(value) <= bound, "{state:?}/{reason:?}");
                        }
                    }
                }
            }
        }
        assert_eq!(valid_pairs, 28);
    }
}

#[tokio::test]
async fn exact_capacity_boundary_is_checked_before_callback() {
    for key in keys() {
        let request = request(&key);
        let bound = crate::responses::encoded_len(&request_projection(&request)).unwrap();
        for capacity in [1, bound - 1] {
            let called = Cell::new(0);
            let result = guarded_request(&request, capacity, || {
                called.set(called.get() + 1);
                async { Ok(opportunity(&request)) }
            })
            .await;
            assert!(matches!(result, Err(Error::RequestTooLarge)));
            assert_eq!(called.get(), 0);
        }
        let called = Cell::new(0);
        let actual = guarded_request(&request, bound, || {
            called.set(called.get() + 1);
            async { Ok(opportunity(&request)) }
        })
        .await
        .unwrap();
        assert_eq!(called.get(), 1);
        assert!(encoded_receipt(actual) <= bound);
    }
}

#[test]
fn donor_bound_is_smaller_than_a_legitimate_long_reason_receipt() {
    let request = request("matrix-1");
    let mut donor = request_projection(&request);
    donor["reason"] = json!(AdvisoryReason::DeterministicInputInvalid);
    let donor_bound = crate::responses::encoded_len(&donor).unwrap();
    let mut actual = opportunity(&request);
    actual.config_revision = i64::MAX;
    actual.matrix_task_revision = Some(i64::MAX);
    assert!(encoded_receipt(actual) > donor_bound);
    assert!(guard_request_output(&request, donor_bound).is_err());
}
