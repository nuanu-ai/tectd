use super::*;

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
fn matrix_verification_stale_is_matrix_only_terminal_invalidation() {
    let reason = AdvisoryReason::MatrixVerificationStale;
    assert_eq!(reason.as_str(), "matrix_verification_stale");
    assert_eq!(
        serde_json::from_str::<AdvisoryReason>("\"matrix_verification_stale\"").unwrap(),
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
fn v2_matrix_no_call_reasons_are_engineering_only_and_state_owned() {
    for reason in [
        AdvisoryReason::MatrixEvidenceUnresolved,
        AdvisoryReason::MatrixSourceUnverified,
        AdvisoryReason::MatrixTaskUnbound,
        AdvisoryReason::MatrixSnapshotMissing,
        AdvisoryReason::MatrixBindingMismatch,
        AdvisoryReason::MatrixContextUnresolved,
        AdvisoryReason::MatrixContextStale,
        AdvisoryReason::MatrixAuthoritySchemaUnsupported,
        AdvisoryReason::MatrixOperatingEvidenceUnresolved,
    ] {
        let encoded = serde_json::to_value(reason).unwrap();
        assert_eq!(encoded, serde_json::json!(reason.as_str()));
        assert_eq!(
            serde_json::from_value::<AdvisoryReason>(encoded).unwrap(),
            reason
        );
        let mut scope = opportunity(AdvisoryRequestPreference::UseWorkspace);
        scope.state = AdvisoryOpportunityState::NoCall;
        scope.primary_reason = reason;
        assert_eq!(scope.validate(), Err(Error::InvalidArguments));
        let mut matrix = scope.clone();
        matrix.capability = AdvisoryCapability::EngineeringProfile;
        matrix.decision_point = AdvisoryDecisionPoint::EngineeringProfileBeforeSelection;
        matrix.target_kind = "matrix_task".into();
        matrix.matrix_task_revision = Some(1);
        assert_eq!(matrix.validate(), Ok(()));
        matrix.matrix_choice_set_digest = Some("a".repeat(64));
        matrix.matrix_verification_digest = Some("b".repeat(64));
        for state in [
            AdvisoryOpportunityState::Prepared,
            AdvisoryOpportunityState::AwaitingResponse,
            AdvisoryOpportunityState::Advised,
            AdvisoryOpportunityState::Invalidated,
            AdvisoryOpportunityState::Failed,
            AdvisoryOpportunityState::Unresolved,
        ] {
            matrix.state = state;
            assert!(!advisory_reason_matches_state(state, reason));
            assert_eq!(matrix.validate(), Err(Error::InvalidArguments));
        }
    }
}
