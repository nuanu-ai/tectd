use super::*;
use tect_domain::{
    AdvisoryDecisionPoint, AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference,
    EngineeringMatrixInput, MatrixFact, OperatingEnvelope, OperationalFacts,
};
use uuid::Uuid;

fn fixture() -> (
    MatrixDecompositionParent,
    crate::MatrixTaskRevision,
    AdvisoryOpportunity,
    Uuid,
) {
    let workspace_id = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let opportunity_id = Uuid::new_v4();
    let parent = MatrixDecompositionParent {
        opportunity_id,
        task_id,
        task_revision: 2,
        input_digest: "a".repeat(64),
        choice_set_digest: Some("b".repeat(64)),
        verification_digest: Some("c".repeat(64)),
        opportunity_material_digest: "d".repeat(64),
        mismatch_rationale: "The current candidate boundary omits required operational work".into(),
    };
    let task = crate::MatrixTaskRevision {
        task_id,
        revision: 2,
        request_id: Uuid::new_v4(),
        input: EngineeringMatrixInput {
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
        },
        input_digest: parent.input_digest.clone(),
        choice_set: None,
        choice_set_digest: parent.choice_set_digest.clone(),
        recorded_by_principal_id: Uuid::new_v4(),
        recorded_by_session_id: Uuid::new_v4(),
    };
    let opportunity = AdvisoryOpportunity {
        id: opportunity_id,
        workspace_id,
        session_id: Uuid::new_v4(),
        authorized_actor_id: Uuid::new_v4(),
        capability: AdvisoryCapability::EngineeringProfile,
        decision_point: AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
        decision_point_version: 1,
        workflow_occurrence_key: Uuid::new_v4().to_string(),
        target_kind: "matrix_task".into(),
        target_id: Some(task_id),
        work_revision: Some(2),
        matrix_task_revision: Some(2),
        matrix_choice_set_digest: parent.choice_set_digest.clone(),
        matrix_verification_digest: parent.verification_digest.clone(),
        source_ref: None,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        config_revision: 1,
        material_digest: parent.opportunity_material_digest.clone(),
        state: AdvisoryOpportunityState::NoCall,
        primary_reason: AdvisoryReason::CapabilityUnavailable,
        provider_called: false,
    };
    (parent, task, opportunity, workspace_id)
}

#[test]
fn exact_current_matrix_parent_is_accepted() {
    let (parent, task, opportunity, workspace) = fixture();
    parent.validate().unwrap();
    validate_matrix_decomposition_parent(&parent, &task, &opportunity, workspace).unwrap();
}

#[test]
fn changed_task_or_foreign_opportunity_is_rejected() {
    let (parent, mut task, mut opportunity, workspace) = fixture();
    task.revision += 1;
    assert!(matches!(
        validate_matrix_decomposition_parent(&parent, &task, &opportunity, workspace),
        Err(Error::StaleRevision)
    ));
    task.revision -= 1;
    opportunity.workspace_id = Uuid::new_v4();
    assert!(matches!(
        validate_matrix_decomposition_parent(&parent, &task, &opportunity, workspace),
        Err(Error::InputConflict)
    ));
    opportunity.workspace_id = workspace;
    opportunity.capability = AdvisoryCapability::ScopeDecomposition;
    assert!(matches!(
        validate_matrix_decomposition_parent(&parent, &task, &opportunity, workspace),
        Err(Error::InputConflict)
    ));
}

#[test]
fn parent_requires_bounded_nonempty_mismatch_reason_and_exact_digests() {
    let (mut parent, _, _, _) = fixture();
    parent.mismatch_rationale = " \t ".into();
    assert!(matches!(parent.validate(), Err(Error::InvalidArguments)));
    parent.mismatch_rationale = "explicit mismatch".into();
    parent.input_digest = "not-a-digest".into();
    assert!(matches!(parent.validate(), Err(Error::InvalidArguments)));
}
