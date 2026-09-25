use super::*;
use crate::{
    GuardedMatrixAdviceRecord, MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy,
    MatrixBudgetRequest, MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt, StoredGuardedMatrixAdviceRecord,
};
use tect_domain::{
    CommitmentEvidence, EngineeringCandidate, EngineeringIntent, EngineeringMode, FactProvenance,
    MATRIX_CHOICE_SET_SCHEMA, MatrixFact, MatrixSourceVerificationStatus, OperatingEnvelope,
    OperatingFact, OperationalFacts,
};

fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("owner report".into()),
    }
}

fn stored_revision() -> MatrixTaskRevision {
    let task_id = Uuid::new_v4();
    let input = EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: MatrixFact::Absent,
            operational_facts: OperationalFacts::Absent,
        },
        criticality: MatrixFact::Absent,
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        affected_guarantees: MatrixFact::Absent,
        actual_exposure: MatrixFact::Absent,
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: MatrixFact::Absent,
    };
    MatrixTaskRevision {
        task_id,
        revision: 2,
        request_id: Uuid::new_v4(),
        input,
        input_digest: "stored-digest".into(),
        choice_set: None,
        choice_set_digest: None,
        recorded_by_principal_id: Uuid::new_v4(),
        recorded_by_session_id: Uuid::new_v4(),
    }
}

fn advisory_request(task_id: Uuid) -> RequestEngineeringAdvisory {
    RequestEngineeringAdvisory {
        task_id,
        expected_task_revision: 2,
        request_key: Uuid::new_v4().to_string(),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    }
}

fn advisory_config(mode: WorkspaceAdvisoryMode) -> WorkspaceAdvisoryConfig {
    WorkspaceAdvisoryConfig {
        workspace_id: Uuid::new_v4(),
        revision: 1,
        mode,
        materialized: true,
        provider_profile_ref: None,
        model_configuration: None,
    }
}

#[test]
fn public_guarded_advice_requires_exact_current_bindings() {
    let task_id = Uuid::new_v4();
    let binding = crate::MatrixProviderBinding {
        task_id,
        task_revision: 2,
        input_digest: "a".repeat(64),
        choice_set_id: "choice".into(),
        choice_set_version: 1,
        choice_set_digest: "b".repeat(64),
        evaluation_digest: "c".repeat(64),
        verification_digest: Some("d".repeat(64)),
    };
    let profile = tect_domain::AdvisoryProviderProfileRef {
        id: "provider".into(),
    };
    let model = tect_domain::AdvisoryModelConfiguration {
        model: "model".into(),
    };
    let mut config = advisory_config(WorkspaceAdvisoryMode::Optional);
    config.provider_profile_ref = Some(profile.clone());
    config.model_configuration = Some(model.clone());
    let mut receipt = AdvisoryOpportunity {
        id: Uuid::new_v4(),
        workspace_id: config.workspace_id,
        session_id: Uuid::new_v4(),
        authorized_actor_id: Uuid::new_v4(),
        capability: AdvisoryCapability::EngineeringProfile,
        decision_point: AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
        decision_point_version: ADVISORY_DECISION_POINT_VERSION,
        workflow_occurrence_key: "key".into(),
        target_kind: "matrix_task".into(),
        target_id: Some(task_id),
        work_revision: Some(2),
        matrix_task_revision: Some(2),
        matrix_choice_set_digest: Some(binding.choice_set_digest.clone()),
        matrix_verification_digest: binding.verification_digest.clone(),
        source_ref: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        config_revision: config.revision,
        material_digest: binding.evaluation_digest.clone(),
        state: AdvisoryOpportunityState::Advised,
        primary_reason: AdvisoryReason::ProviderResponse,
        provider_called: true,
    };
    for outcome in [
        crate::GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["a".into(), "b".into()],
        },
        crate::GuardedMatrixAdviceOutcome::Abstained { reason: None },
    ] {
        let record = GuardedMatrixAdviceRecord {
            opportunity_id: receipt.id,
            dispatch_id: Uuid::new_v4(),
            opportunity_material_digest: binding.evaluation_digest.clone(),
            binding: binding.clone(),
            provider_profile_ref: profile.clone(),
            model_configuration: model.clone(),
            raw_response_payload: Vec::new(),
            response_payload_sha256: "e".repeat(64),
            advice_digest: "f".repeat(64),
            outcome: outcome.clone(),
        };
        let stored = StoredGuardedMatrixAdviceRecord {
            advice_id: Uuid::new_v4(),
            record,
        };
        let current =
            current_public_matrix_advice(&receipt, &stored, &config, Some(&binding)).unwrap();
        assert_eq!(current.outcome, outcome);
        assert_eq!(current.advice_id, stored.advice_id);
        assert_eq!(current.verification_digest, "d".repeat(64));
        assert!(current_public_matrix_advice(&receipt, &stored, &config, None).is_none());
        let mut changed_binding = binding.clone();
        changed_binding.choice_set_digest = "0".repeat(64);
        assert!(
            current_public_matrix_advice(&receipt, &stored, &config, Some(&changed_binding))
                .is_none()
        );
        changed_binding = binding.clone();
        changed_binding.verification_digest = Some("0".repeat(64));
        assert!(
            current_public_matrix_advice(&receipt, &stored, &config, Some(&changed_binding))
                .is_none()
        );
        let mut changed_config = config.clone();
        changed_config.revision += 1;
        assert!(
            current_public_matrix_advice(&receipt, &stored, &changed_config, Some(&binding))
                .is_none()
        );
        receipt.state = AdvisoryOpportunityState::NoCall;
        assert!(current_public_matrix_advice(&receipt, &stored, &config, Some(&binding)).is_none());
        receipt.state = AdvisoryOpportunityState::Advised;
    }
    let record = GuardedMatrixAdviceRecord {
        opportunity_id: receipt.id,
        dispatch_id: Uuid::new_v4(),
        opportunity_material_digest: binding.evaluation_digest.clone(),
        binding: binding.clone(),
        provider_profile_ref: profile,
        model_configuration: model,
        raw_response_payload: Vec::new(),
        response_payload_sha256: "e".repeat(64),
        advice_digest: "f".repeat(64),
        outcome: crate::GuardedMatrixAdviceOutcome::Rejected {
            reason: "invalid".into(),
        },
    };
    let stored = StoredGuardedMatrixAdviceRecord {
        advice_id: Uuid::new_v4(),
        record,
    };
    assert!(current_public_matrix_advice(&receipt, &stored, &config, Some(&binding)).is_none());
}

struct TestProvider(MatrixProviderIdentity);

#[async_trait::async_trait]
impl MatrixAdviceProvider for TestProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        Some(self.0.clone())
    }

    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        let _ = request;
        panic!("unverified Matrix must not prepare a provider attempt")
    }

    async fn attempt_prepared(
        &self,
        _: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        panic!("request capture must not send")
    }
}

struct TestBudget(bool);

#[async_trait::async_trait]
impl MatrixBudgetPolicy for TestBudget {
    async fn authorize(
        &self,
        _: &MatrixBudgetRequest,
        policy: &tect_domain::AdvisoryBudgetPolicy,
    ) -> Result<Option<MatrixBudgetAuthorization>> {
        Ok(self.0.then(|| MatrixBudgetAuthorization {
            policy_id: policy.id().to_string(),
        }))
    }
}

#[path = "tests/owner_reported.rs"]
mod owner_reported;

#[path = "tests/no_call.rs"]
mod no_call;

#[path = "tests/composition.rs"]
mod composition;
