use super::*;
use tect_domain::{AdvisoryRequestPreference, MandatoryMatrixCard, MatrixSourceVerificationStatus};

fn opportunity() -> AdvisoryOpportunity {
    AdvisoryOpportunity {
        id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        authorized_actor_id: Uuid::new_v4(),
        capability: AdvisoryCapability::EngineeringProfile,
        decision_point: AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
        decision_point_version: 1,
        workflow_occurrence_key: "skip-verified".into(),
        target_kind: "matrix_task".into(),
        target_id: Some(Uuid::new_v4()),
        work_revision: Some(1),
        matrix_task_revision: Some(1),
        matrix_choice_set_digest: Some("b".repeat(64)),
        matrix_verification_digest: Some("a".repeat(64)),
        source_ref: None,
        session_preference: AdvisoryRequestPreference::Skip,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        config_revision: 1,
        material_digest: "c".repeat(64),
        state: AdvisoryOpportunityState::NoCall,
        primary_reason: AdvisoryReason::SessionSkip,
        provider_called: false,
    }
}

fn composition() -> EngineeringMatrixComposition {
    EngineeringMatrixComposition {
        catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
        task_id: "task".into(),
        task_revision: "1".into(),
        source_verification_status:
            MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported,
        mandatory_cards: vec![MandatoryMatrixCard {
            id: "EM02-SCOPE@0.1",
            catalogue_version: ENGINEERING_MATRIX_CATALOGUE_VERSION,
            summary: "Scope",
            body: "Mandatory scope",
        }],
        unresolved_evidence: Vec::new(),
    }
}

#[derive(Default)]
struct FakeDispositionPort {
    inserts: usize,
}

impl FakeDispositionPort {
    fn select(
        &mut self,
        receipt: &AdvisoryOpportunity,
        cards: &EngineeringMatrixComposition,
    ) -> Result<()> {
        selected_snapshot_current(receipt, cards, &"a".repeat(64), false)?;
        self.inserts += 1;
        Ok(())
    }
}

#[test]
fn historical_unverified_no_call_cannot_select_even_after_evidence_improves() {
    let mut port = FakeDispositionPort::default();
    for reason in [
        AdvisoryReason::MatrixEvidenceUnresolved,
        AdvisoryReason::MatrixSourceUnverified,
    ] {
        let mut receipt = opportunity();
        receipt.primary_reason = reason;
        receipt.matrix_verification_digest = None;
        assert_eq!(
            port.select(&receipt, &composition()),
            Err(Error::StaleContext)
        );
    }
    assert_eq!(port.inserts, 0);
}

#[test]
fn verified_optional_advice_skip_can_select_but_lost_cards_or_digest_cannot() {
    let mut port = FakeDispositionPort::default();
    let receipt = opportunity();
    port.select(&receipt, &composition()).unwrap();
    assert_eq!(port.inserts, 1);
    let mut stale = receipt.clone();
    stale.matrix_verification_digest = None;
    assert_eq!(
        port.select(&stale, &composition()),
        Err(Error::StaleContext)
    );
    let mut missing_card = composition();
    missing_card.mandatory_cards.clear();
    assert_eq!(
        port.select(&receipt, &missing_card),
        Err(Error::StaleContext)
    );
    assert_eq!(port.inserts, 1);
}

#[test]
fn context_authority_accepts_confirmed_requirements_without_legacy_owner_report_status() {
    let receipt = opportunity();
    let mut confirmed = composition();
    confirmed.source_verification_status =
        MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification;
    assert_eq!(
        selected_snapshot_current(&receipt, &confirmed, &"a".repeat(64), false),
        Err(Error::StaleContext)
    );
    selected_snapshot_current(&receipt, &confirmed, &"a".repeat(64), true).unwrap();

    let mut stale = receipt;
    stale.matrix_verification_digest = Some("d".repeat(64));
    assert_eq!(
        selected_snapshot_current(&stale, &confirmed, &"a".repeat(64), true),
        Err(Error::StaleContext)
    );
    confirmed.mandatory_cards.clear();
    assert_eq!(
        selected_snapshot_current(&opportunity(), &confirmed, &"a".repeat(64), true),
        Err(Error::StaleContext)
    );
}

mod conditional_cards;

fn blocked_request() -> RecordMatrixDisposition {
    RecordMatrixDisposition {
        request_id: Uuid::new_v4(),
        task_id: Uuid::new_v4(),
        expected_task_revision: 1,
        expected_input_digest: "a".repeat(64),
        expected_choice_set_digest: Some("b".repeat(64)),
        opportunity_id: Uuid::new_v4(),
        basis: MatrixDispositionBasis::Manual,
        advice_id: None,
        advice_digest: None,
        decision: MatrixDispositionDecision::Blocked {
            blocked_reason: "Explicit blocker".into(),
        },
    }
}

#[test]
fn blocked_after_advice_keeps_required_current_verification() {
    let mut request = blocked_request();
    assert!(!requires_current_verification(&request));
    request.basis = MatrixDispositionBasis::AfterAdvice;
    request.advice_id = Some(Uuid::new_v4());
    request.advice_digest = Some("c".repeat(64));
    request.validate().unwrap();
    assert!(requires_current_verification(&request));
    request.basis = MatrixDispositionBasis::NoCall;
    request.advice_id = None;
    request.advice_digest = None;
    request.decision = MatrixDispositionDecision::Selected {
        selected_choice_id: "owner-choice".into(),
    };
    request.validate().unwrap();
    assert!(requires_current_verification(&request));
}

#[test]
fn independent_verifier_reads_fact_but_owner_remains_creator_session_bound() {
    let principal = Uuid::new_v4();
    let session = Uuid::new_v4();
    let request = blocked_request();
    let saved = MatrixDispositionRecord {
        disposition_id: Uuid::new_v4(),
        material_digest: request.material_digest().unwrap(),
        request,
        recorded_by_principal_id: principal,
        recorded_by_session_id: session,
    };
    let task = saved.request.task_id;
    assert!(disposition_visible_to_reader(
        &saved,
        task,
        PrincipalRole::Owner,
        Some((principal, session))
    ));
    assert!(!disposition_visible_to_reader(
        &saved,
        task,
        PrincipalRole::Owner,
        Some((principal, Uuid::new_v4()))
    ));
    assert!(!disposition_visible_to_reader(
        &saved,
        task,
        PrincipalRole::Owner,
        Some((Uuid::new_v4(), session))
    ));
    assert!(!disposition_visible_to_reader(
        &saved,
        task,
        PrincipalRole::Owner,
        None
    ));
    assert!(disposition_visible_to_reader(
        &saved,
        task,
        PrincipalRole::Verifier,
        None
    ));
    assert!(!disposition_visible_to_reader(
        &saved,
        Uuid::new_v4(),
        PrincipalRole::Verifier,
        None
    ));
}

#[test]
fn context_singleton_no_call_selection_requires_captured_current_verification() {
    let mut receipt = opportunity();
    receipt.primary_reason = AdvisoryReason::ChoiceSetNotApplicable;
    let mut confirmed = composition();
    confirmed.source_verification_status =
        MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification;
    selected_snapshot_current(&receipt, &confirmed, &"a".repeat(64), true).unwrap();
    receipt.matrix_verification_digest = None;
    assert_eq!(
        selected_snapshot_current(&receipt, &confirmed, &"a".repeat(64), true),
        Err(Error::StaleContext)
    );
    receipt.matrix_verification_digest = Some("b".repeat(64));
    assert_eq!(
        selected_snapshot_current(&receipt, &confirmed, &"a".repeat(64), true),
        Err(Error::StaleContext)
    );
}
