use super::*;
use crate::{DisabledMatrixAdviceProvider, MatrixAdviceProvider};
use tect_domain::{
    AdvisoryDecisionPoint, AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference,
};

#[test]
fn committed_permit_matches_only_its_prepared_attempt() {
    let binding = MatrixProviderBinding {
        task_id: Uuid::new_v4(),
        task_revision: 1,
        input_digest: "a".repeat(64),
        choice_set_id: "choice".into(),
        choice_set_version: 1,
        choice_set_digest: "b".repeat(64),
        evaluation_digest: "c".repeat(64),
        verification_digest: None,
    };
    let identity = MatrixProviderIdentity {
        provider_profile_ref: AdvisoryProviderProfileRef { id: "test".into() },
        model_configuration: AdvisoryModelConfiguration {
            model: "test-model".into(),
        },
        destination: "test-target".into(),
        wire_version: "test-wire/1".into(),
    };
    let prepared = PreparedMatrixAdviceAttempt {
        binding: binding.clone(),
        identity: identity.clone(),
        body: b"exact-body".to_vec(),
        body_sha256: format!("{:x}", Sha256::digest(b"exact-body")),
    };
    let permit = || MatrixStartedDispatchPermit {
        opportunity_id: Uuid::new_v4(),
        dispatch_id: Uuid::new_v4(),
        configuration_digest: "d".repeat(64),
        binding: binding.clone(),
        identity: identity.clone(),
        body_length: prepared.body_length(),
        body_sha256: prepared.body_sha256.clone(),
    };
    assert!(permit().permits_prepared(&prepared));

    let mut changed = PreparedMatrixAdviceAttempt {
        binding: binding.clone(),
        identity: identity.clone(),
        body: b"other-body".to_vec(),
        body_sha256: format!("{:x}", Sha256::digest(b"other-body")),
    };
    assert!(!permit().permits_prepared(&changed));
    changed = PreparedMatrixAdviceAttempt {
        binding: binding.clone(),
        identity: MatrixProviderIdentity {
            destination: "other-target".into(),
            ..identity.clone()
        },
        body: prepared.body.clone(),
        body_sha256: prepared.body_sha256.clone(),
    };
    assert!(!permit().permits_prepared(&changed));
    changed.identity = identity.clone();
    changed.binding.task_id = Uuid::new_v4();
    assert!(!permit().permits_prepared(&changed));
}

#[test]
fn dispatch_permit_binding_rejects_different_evaluation_material() {
    let binding = MatrixProviderBinding {
        task_id: Uuid::new_v4(),
        task_revision: 2,
        input_digest: "a".repeat(64),
        choice_set_id: "choice".into(),
        choice_set_version: 1,
        choice_set_digest: "b".repeat(64),
        evaluation_digest: "c".repeat(64),
        verification_digest: None,
    };
    let mut opportunity = AdvisoryOpportunity {
        id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        authorized_actor_id: Uuid::new_v4(),
        capability: AdvisoryCapability::EngineeringProfile,
        decision_point: AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
        decision_point_version: 1,
        workflow_occurrence_key: "key".into(),
        target_kind: "matrix_task".into(),
        target_id: Some(binding.task_id),
        work_revision: Some(binding.task_revision),
        matrix_task_revision: Some(binding.task_revision),
        matrix_choice_set_digest: Some(binding.choice_set_digest.clone()),
        matrix_verification_digest: binding.verification_digest.clone(),
        source_ref: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        config_revision: 1,
        material_digest: binding.evaluation_digest.clone(),
        state: AdvisoryOpportunityState::Prepared,
        primary_reason: AdvisoryReason::DispatchAuthorized,
        provider_called: false,
    };
    assert!(binding_matches_opportunity(&binding, &opportunity));
    opportunity.material_digest = "d".repeat(64);
    assert!(!binding_matches_opportunity(&binding, &opportunity));
}

#[tokio::test]
async fn disabled_identity_and_explicit_budget_deny() {
    let provider = DisabledMatrixAdviceProvider;
    assert_eq!(provider.identity(), None);
    let budget = DenyMatrixBudget;
    let request = MatrixBudgetRequest {
        workspace_id: Uuid::new_v4(),
        actor_id: Uuid::new_v4(),
        binding: MatrixProviderBinding {
            task_id: Uuid::new_v4(),
            task_revision: 1,
            input_digest: "input".into(),
            choice_set_id: "choice".into(),
            choice_set_version: 1,
            choice_set_digest: "choice-digest".into(),
            evaluation_digest: "evaluation".into(),
            verification_digest: None,
        },
        provider_profile_ref: AdvisoryProviderProfileRef { id: "test".into() },
        model_configuration: AdvisoryModelConfiguration {
            model: "test".into(),
        },
        destination: "test-target".into(),
        wire_version: "test-wire/1".into(),
        body_length: 2,
        body_sha256: "digest".into(),
    };
    let id = Uuid::new_v4();
    let ceilings = tect_domain::AdvisoryBudgetCeilings {
        provider_calls: 1,
        input_tokens: 1,
        output_tokens: 1,
        request_utf8_bytes: 2,
        elapsed_monotonic_ms: 1,
        retry_dispatches: 1,
    };
    let policy = AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, 0, 100, ceilings),
        0,
        100,
        ceilings,
        Uuid::new_v4(),
        "a".repeat(128),
    )
    .unwrap();
    assert_eq!(budget.authorize(&request, &policy).await, Ok(None));
    assert_eq!(
        SignedMatrixBudgetPreflight
            .authorize(&request, &policy)
            .await,
        Ok(Some(MatrixBudgetAuthorization {
            policy_id: id.to_string()
        }))
    );
    let too_large = MatrixBudgetRequest {
        body_length: 3,
        ..request
    };
    assert_eq!(
        SignedMatrixBudgetPreflight
            .authorize(&too_large, &policy)
            .await,
        Ok(None)
    );
}
