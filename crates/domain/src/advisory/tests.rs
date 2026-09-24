use super::*;
use crate::Error;
use uuid::Uuid;

fn opportunity(preference: AdvisoryRequestPreference) -> AdvisoryOpportunityInput {
    let decision = assess_advisory_opportunity(WorkspaceAdvisoryMode::Optional, preference);
    AdvisoryOpportunityInput {
        session_id: Uuid::new_v4(),
        authorized_actor_id: Uuid::new_v4(),
        capability: AdvisoryCapability::ScopeDecomposition,
        decision_point: AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
        decision_point_version: ADVISORY_DECISION_POINT_VERSION,
        workflow_occurrence_key: Uuid::new_v4().to_string(),
        target_kind: "program".into(),
        target_id: Some(Uuid::new_v4()),
        work_revision: Some(1),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        matrix_verification_digest: None,
        source_ref: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: preference,
        config_revision: 1,
        material_digest: "a".repeat(64),
        state: decision.state,
        primary_reason: decision.reason,
    }
}

#[test]
fn every_preference_combination_only_narrows_workspace_permission() {
    for mode in [
        WorkspaceAdvisoryMode::Disabled,
        WorkspaceAdvisoryMode::Optional,
    ] {
        for session in [
            AdvisoryRequestPreference::UseWorkspace,
            AdvisoryRequestPreference::Skip,
        ] {
            for request in [
                AdvisoryRequestPreference::UseWorkspace,
                AdvisoryRequestPreference::Skip,
            ] {
                let decision = assess_advisory_policy(AdvisoryPolicyInput {
                    workspace_mode: mode,
                    session_preference: session,
                    request_preference: request,
                    deterministic_input_valid: true,
                    capability_available: true,
                    provider_configured: true,
                });
                let permitted = mode == WorkspaceAdvisoryMode::Optional
                    && session == AdvisoryRequestPreference::UseWorkspace
                    && request == AdvisoryRequestPreference::UseWorkspace;
                assert_eq!(
                    decision.state == AdvisoryOpportunityState::Prepared,
                    permitted
                );
                if mode == WorkspaceAdvisoryMode::Disabled {
                    assert_eq!(decision.reason, AdvisoryReason::WorkspaceDisabled);
                } else if session == AdvisoryRequestPreference::Skip {
                    assert_eq!(decision.reason, AdvisoryReason::SessionSkip);
                } else if request == AdvisoryRequestPreference::Skip {
                    assert_eq!(decision.reason, AdvisoryReason::RequestSkip);
                }
            }
        }
    }
}

#[test]
fn no_call_reason_must_match_the_narrowing_preference() {
    let mut input = opportunity(AdvisoryRequestPreference::Skip);
    assert!(input.validate().is_ok());
    input.request_preference = AdvisoryRequestPreference::UseWorkspace;
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
}

#[test]
fn engineering_profile_requires_exact_typed_matrix_binding() {
    let mut input = opportunity(AdvisoryRequestPreference::UseWorkspace);
    input.capability = AdvisoryCapability::EngineeringProfile;
    input.decision_point = AdvisoryDecisionPoint::EngineeringProfileBeforeSelection;
    input.target_kind = "matrix_task".into();
    input.matrix_task_revision = Some(1);
    input.state = AdvisoryOpportunityState::NoCall;
    input.primary_reason = AdvisoryReason::ChoiceSetNotApplicable;
    assert!(input.validate().is_ok());
    input.work_revision = Some(2);
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
    input.work_revision = Some(1);
    input.matrix_choice_set_digest = Some("not-a-sha".into());
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
    input.matrix_choice_set_digest = None;
    input.state = AdvisoryOpportunityState::Prepared;
    input.primary_reason = AdvisoryReason::DispatchAuthorized;
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
    input.matrix_choice_set_digest = Some("a".repeat(64));
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
    input.matrix_verification_digest = Some("not-a-sha".into());
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
    input.matrix_verification_digest = Some("b".repeat(64));
    assert!(input.validate().is_ok());
}

#[test]
fn budget_policy_invalid_is_a_typed_no_call_reason() {
    let reason = AdvisoryReason::BudgetPolicyInvalid;
    assert_eq!(reason.as_str(), "budget_policy_invalid");
    assert_eq!(
        serde_json::to_value(reason).unwrap(),
        "budget_policy_invalid"
    );
    assert_eq!(
        serde_json::from_str::<AdvisoryReason>("\"budget_policy_invalid\"").unwrap(),
        reason
    );
    assert!(advisory_reason_matches_state(
        AdvisoryOpportunityState::NoCall,
        reason
    ));
    assert!(!advisory_reason_matches_state(
        AdvisoryOpportunityState::Prepared,
        reason
    ));
}

#[test]
fn choice_set_not_applicable_is_a_typed_no_call_reason() {
    let reason = AdvisoryReason::ChoiceSetNotApplicable;
    assert_eq!(reason.as_str(), "choice_set_not_applicable");
    assert_eq!(
        serde_json::to_value(reason).unwrap(),
        "choice_set_not_applicable"
    );
    assert_eq!(
        serde_json::from_str::<AdvisoryReason>("\"choice_set_not_applicable\"").unwrap(),
        reason
    );
    assert!(advisory_reason_matches_state(
        AdvisoryOpportunityState::NoCall,
        reason
    ));
    assert!(!advisory_reason_matches_state(
        AdvisoryOpportunityState::Prepared,
        reason
    ));
}

#[test]
fn matrix_revision_change_is_matrix_only_invalidation() {
    let reason = AdvisoryReason::MatrixTaskRevisionChanged;
    assert_eq!(reason.as_str(), "matrix_task_revision_changed");
    assert_eq!(
        serde_json::to_value(reason).unwrap(),
        "matrix_task_revision_changed"
    );
    assert_eq!(
        serde_json::from_str::<AdvisoryReason>("\"matrix_task_revision_changed\"").unwrap(),
        reason
    );
    assert!(advisory_reason_matches_state(
        AdvisoryOpportunityState::Invalidated,
        reason
    ));
    assert!(!advisory_reason_matches_state(
        AdvisoryOpportunityState::NoCall,
        reason
    ));

    let mut input = opportunity(AdvisoryRequestPreference::UseWorkspace);
    input.state = AdvisoryOpportunityState::Invalidated;
    input.primary_reason = reason;
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
    input.capability = AdvisoryCapability::EngineeringProfile;
    input.decision_point = AdvisoryDecisionPoint::EngineeringProfileBeforeSelection;
    input.target_kind = "matrix_task".into();
    input.matrix_task_revision = Some(1);
    input.matrix_choice_set_digest = Some("b".repeat(64));
    assert!(input.validate().is_ok());
}

#[test]
fn reason_precedence_is_deterministic() {
    let base = AdvisoryPolicyInput {
        workspace_mode: WorkspaceAdvisoryMode::Optional,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        deterministic_input_valid: true,
        capability_available: true,
        provider_configured: true,
    };
    assert_eq!(
        assess_advisory_policy(AdvisoryPolicyInput {
            workspace_mode: WorkspaceAdvisoryMode::Disabled,
            session_preference: AdvisoryRequestPreference::Skip,
            request_preference: AdvisoryRequestPreference::Skip,
            deterministic_input_valid: false,
            capability_available: false,
            provider_configured: false,
        })
        .reason,
        AdvisoryReason::WorkspaceDisabled
    );
    assert_eq!(
        assess_advisory_policy(AdvisoryPolicyInput {
            deterministic_input_valid: false,
            capability_available: false,
            provider_configured: false,
            ..base
        })
        .reason,
        AdvisoryReason::DeterministicInputInvalid
    );
    assert_eq!(
        assess_advisory_policy(AdvisoryPolicyInput {
            capability_available: false,
            provider_configured: false,
            ..base
        })
        .reason,
        AdvisoryReason::CapabilityUnavailable
    );
    assert_eq!(
        assess_advisory_policy(AdvisoryPolicyInput {
            provider_configured: false,
            ..base
        })
        .reason,
        AdvisoryReason::ProviderUnconfigured
    );
}

#[test]
fn state_transitions_are_closed() {
    assert!(AdvisoryOpportunityState::Prepared.can_transition_to(AdvisoryOpportunityState::NoCall));
    assert!(
        AdvisoryOpportunityState::AwaitingResponse
            .can_transition_to(AdvisoryOpportunityState::Unresolved)
    );
    assert!(
        !AdvisoryOpportunityState::NoCall
            .can_transition_to(AdvisoryOpportunityState::AwaitingResponse)
    );
    assert!(AdvisoryDispatchState::Authorized.can_transition_to(AdvisoryDispatchState::Sending));
    assert!(AdvisoryDispatchState::Authorized.can_transition_to(AdvisoryDispatchState::Cancelled));
    assert!(!AdvisoryDispatchState::Sending.can_transition_to(AdvisoryDispatchState::Cancelled));
}

#[test]
fn sent_unknown_forbids_blind_retry() {
    assert!(!advisory_retry_permitted(
        AdvisorySendCertainty::SentUnknown,
        AdvisoryRetryBasis::ProvenNotSent,
    ));
    assert!(!advisory_retry_permitted(
        AdvisorySendCertainty::SentUnknown,
        AdvisoryRetryBasis::KnownRetryableResponse,
    ));
    assert!(!advisory_retry_permitted(
        AdvisorySendCertainty::SentUnknown,
        AdvisoryRetryBasis::VerifiedProviderIdempotency,
    ));
    assert!(!advisory_retry_permitted(
        AdvisorySendCertainty::Sent,
        AdvisoryRetryBasis::KnownRetryableResponse,
    ));
    assert!(advisory_retry_permitted(
        AdvisorySendCertainty::NotSent,
        AdvisoryRetryBasis::ProvenNotSent,
    ));
}

#[test]
fn revision_zero_advances_once_and_stale_expected_revision_fails() {
    assert_eq!(next_advisory_config_revision(None, 0), Ok(1));
    assert_eq!(next_advisory_config_revision(Some(3), 3), Ok(4));
    assert_eq!(
        next_advisory_config_revision(Some(3), 2),
        Err(Error::StaleRevision)
    );
}

#[test]
fn configuration_is_typed_non_secret_and_provider_pair_is_atomic() {
    let unconfigured = ConfigureWorkspaceAdvisory {
        expected_revision: 0,
        mode: WorkspaceAdvisoryMode::Optional,
        provider_profile_ref: None,
        model_configuration: None,
    };
    assert_eq!(unconfigured.validate(), Ok(()));

    let configured = ConfigureWorkspaceAdvisory {
        provider_profile_ref: Some(AdvisoryProviderProfileRef {
            id: "jev-production".into(),
        }),
        model_configuration: Some(AdvisoryModelConfiguration {
            model: "jev-advisory-v1".into(),
        }),
        ..unconfigured.clone()
    };
    assert_eq!(configured.validate(), Ok(()));
    let projected = WorkspaceAdvisoryConfig {
        workspace_id: Uuid::new_v4(),
        revision: 1,
        mode: WorkspaceAdvisoryMode::Optional,
        materialized: true,
        provider_profile_ref: configured.provider_profile_ref.clone(),
        model_configuration: configured.model_configuration.clone(),
    };
    assert!(projected.provider_configured());
    assert_eq!(
        assess_advisory_policy(AdvisoryPolicyInput {
            workspace_mode: WorkspaceAdvisoryMode::Optional,
            session_preference: AdvisoryRequestPreference::UseWorkspace,
            request_preference: AdvisoryRequestPreference::UseWorkspace,
            deterministic_input_valid: true,
            capability_available: true,
            provider_configured: false,
        })
        .reason,
        AdvisoryReason::ProviderUnconfigured
    );

    let incomplete = ConfigureWorkspaceAdvisory {
        model_configuration: None,
        ..configured
    };
    assert_eq!(incomplete.validate(), Err(Error::InvalidArguments));
    assert!(
        serde_json::from_value::<ConfigureWorkspaceAdvisory>(serde_json::json!({
            "expected_revision": 0,
            "mode": "optional",
            "provider_api_key": "must-not-enter-workspace-config"
        }))
        .is_err()
    );
}

#[test]
fn registered_decision_point_is_closed_and_capability_owned() {
    assert!(
        AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection
            .supports(AdvisoryCapability::ScopeDecomposition)
    );
    assert!(
        !AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection
            .supports(AdvisoryCapability::PipelineRecommendation)
    );
    let engineering = AdvisoryDecisionPoint::EngineeringProfileBeforeSelection;
    assert_eq!(engineering.as_str(), ENGINEERING_PROFILE_DECISION_POINT);
    assert_eq!(engineering.to_string(), ENGINEERING_PROFILE_DECISION_POINT);
    assert_eq!(
        engineering.capability(),
        AdvisoryCapability::EngineeringProfile
    );
    assert!(engineering.supports(AdvisoryCapability::EngineeringProfile));
    assert!(!engineering.supports(AdvisoryCapability::ScopeDecomposition));
    assert_eq!(
        ENGINEERING_PROFILE_DECISION_POINT.parse::<AdvisoryDecisionPoint>(),
        Ok(engineering)
    );
    assert_eq!(
        serde_json::to_value(engineering).unwrap(),
        serde_json::json!(ENGINEERING_PROFILE_DECISION_POINT)
    );
    assert_eq!(
        serde_json::from_value::<AdvisoryDecisionPoint>(serde_json::json!(
            ENGINEERING_PROFILE_DECISION_POINT
        ))
        .unwrap(),
        engineering
    );
    assert_eq!(
        SCOPE_DECOMPOSITION_DECISION_POINT.parse::<AdvisoryDecisionPoint>(),
        Ok(AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection)
    );
    assert!(
        "engineering.profile.unknown"
            .parse::<AdvisoryDecisionPoint>()
            .is_err()
    );
    let mut input = opportunity(AdvisoryRequestPreference::UseWorkspace);
    input.capability = AdvisoryCapability::PipelineRecommendation;
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
    input.capability = AdvisoryCapability::EngineeringProfile;
    input.decision_point = engineering;
    input.target_kind = "matrix_task".into();
    input.matrix_task_revision = input.work_revision;
    input.matrix_choice_set_digest = Some("b".repeat(64));
    assert!(input.validate().is_ok());
    input.capability = AdvisoryCapability::ScopeDecomposition;
    assert_eq!(input.validate(), Err(Error::InvalidArguments));
    assert!(
        serde_json::from_value::<AdvisoryDecisionPoint>(serde_json::json!(
            "pipeline.before_execution"
        ))
        .is_err()
    );
}

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
