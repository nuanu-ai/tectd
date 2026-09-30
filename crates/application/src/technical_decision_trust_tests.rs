use super::*;
use tect_domain::{
    DeliveryApproach, DeliveryApproachKind, TechnicalApprovalAuthority, TechnicalFactValue,
    TechnicalSourceSupport, compare_delivery_mechanisms_with_trust,
};

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fact(kind: TechnicalFactKind, value: TechnicalFactValue) -> TechnicalDecisionFact {
    TechnicalDecisionFact {
        kind,
        observation: TechnicalFactObservation::Verified {
            value,
            binding: TechnicalEvidenceBinding {
                task_id: "task-1".into(),
                task_revision: "2".into(),
                matrix_verification_digest: DIGEST.into(),
                evidence_ref: "approved-artifact@1".into(),
                content_digest: DIGEST.into(),
                validator_policy_version: "test-typed-source@1".into(),
                observed_at: 10,
                expires_at: 30,
                validation_outcome: EvidenceValidationOutcome::Accepted,
            },
        },
    }
}

fn card() -> DeliveryMechanismDecisionCard {
    let mut card = DeliveryMechanismDecisionCard {
        schema: tect_domain::TECHNICAL_DECISION_SCHEMA.into(),
        card_id: tect_domain::DELIVERY_MECHANISM_CARD.into(),
        task_id: "task-1".into(),
        task_revision: "2".into(),
        matrix_verification_digest: DIGEST.into(),
        decision_question: "Which path delivers the agreed updates?".into(),
        required_outcome: "Actual updates with recovery proof".into(),
        approaches: vec![
            DeliveryApproach {
                id: "reuse".into(),
                kind: DeliveryApproachKind::ReuseExistingPath,
                title: "Existing path".into(),
                mechanism: "Existing runtime".into(),
                operational_consequences: vec!["Use current recovery path".into()],
            },
            DeliveryApproach {
                id: "separate".into(),
                kind: DeliveryApproachKind::SeparateMechanism,
                title: "Separate path".into(),
                mechanism: "New runtime".into(),
                operational_consequences: vec!["Operate a second recovery path".into()],
            },
        ],
        owner_approval: TechnicalOwnerApprovalClaim {
            approval_ref: "saved-choice-set@2".into(),
            approving_principal: "owner-principal".into(),
            authority: TechnicalApprovalAuthority::Owner,
            task_id: "task-1".into(),
            task_revision: "2".into(),
            candidate_digest: String::new(),
            approved_at: 12,
        },
        facts: vec![
            fact(
                TechnicalFactKind::ReuseSourceSupport,
                TechnicalFactValue::SourceSupport(TechnicalSourceSupport::Supported),
            ),
            fact(
                TechnicalFactKind::SeparateSourceSupport,
                TechnicalFactValue::SourceSupport(TechnicalSourceSupport::Supported),
            ),
            fact(
                TechnicalFactKind::ReuseMeetsOutcome,
                TechnicalFactValue::Determination(true),
            ),
            fact(
                TechnicalFactKind::ReuseOperationsAcceptable,
                TechnicalFactValue::Determination(true),
            ),
            fact(
                TechnicalFactKind::SeparateMeetsOutcome,
                TechnicalFactValue::Determination(true),
            ),
            fact(
                TechnicalFactKind::SeparateOperationsAcceptable,
                TechnicalFactValue::Determination(true),
            ),
            fact(
                TechnicalFactKind::SeparateRequiredByConstraint,
                TechnicalFactValue::Determination(false),
            ),
        ],
    };
    card.owner_approval.candidate_digest = card.candidate_digest().unwrap();
    card
}

struct FakeSource {
    facts: BTreeMap<TechnicalFactKind, TechnicalDecisionFact>,
    approval: Option<TechnicalOwnerApprovalClaim>,
}

impl FakeSource {
    fn from_card(card: &DeliveryMechanismDecisionCard) -> Self {
        Self {
            facts: card
                .facts
                .iter()
                .cloned()
                .map(|fact| (fact.kind, fact))
                .collect(),
            approval: Some(card.owner_approval.clone()),
        }
    }
}

#[async_trait]
impl TechnicalDecisionSnapshotSource for FakeSource {
    async fn resolve_fact(
        &self,
        _: &DeliveryMechanismDecisionCard,
        kind: TechnicalFactKind,
        _: i64,
    ) -> Result<Option<TechnicalDecisionFact>> {
        Ok(self.facts.get(&kind).cloned())
    }

    async fn resolve_owner_approval(
        &self,
        _: &DeliveryMechanismDecisionCard,
        _: &str,
        _: i64,
    ) -> Result<Option<TechnicalOwnerApprovalClaim>> {
        Ok(self.approval.clone())
    }
}

#[tokio::test]
async fn typed_source_snapshot_enables_only_exact_current_comparison() {
    let card = card();
    let source = FakeSource::from_card(&card);
    let trust = build_technical_decision_trust_snapshot(&source, &card, 20)
        .await
        .unwrap();
    let comparison =
        compare_delivery_mechanisms_with_trust(&card, "task-1", "2", DIGEST, 20, &trust).unwrap();
    assert_eq!(comparison.eligible_approach_ids, ["reuse"]);
    assert!(
        compare_delivery_mechanisms_with_trust(&card, "task-1", "2", DIGEST, 21, &trust,).is_err()
    );
}

#[tokio::test]
async fn unconfigured_or_incomplete_source_denies_even_accepted_claims() {
    let card = card();
    let empty = FakeSource {
        facts: BTreeMap::new(),
        approval: None,
    };
    assert!(
        build_technical_decision_trust_snapshot(&empty, &card, 20)
            .await
            .is_err()
    );

    let mut missing = FakeSource::from_card(&card);
    missing.facts.remove(&TechnicalFactKind::ReuseMeetsOutcome);
    assert!(
        build_technical_decision_trust_snapshot(&missing, &card, 20)
            .await
            .is_err()
    );

    let mut wrong_owner = FakeSource::from_card(&card);
    wrong_owner.approval.as_mut().unwrap().approving_principal = "another-owner".into();
    let trust = build_technical_decision_trust_snapshot(&wrong_owner, &card, 20)
        .await
        .unwrap();
    assert!(
        compare_delivery_mechanisms_with_trust(&card, "task-1", "2", DIGEST, 20, &trust,).is_err()
    );
}

#[tokio::test]
async fn stale_binding_or_changed_fact_is_denied() {
    let card = card();
    let mut source = FakeSource::from_card(&card);
    let saved = source
        .facts
        .get_mut(&TechnicalFactKind::ReuseMeetsOutcome)
        .unwrap();
    let TechnicalFactObservation::Verified { binding, .. } = &mut saved.observation else {
        unreachable!()
    };
    binding.expires_at = 20;
    assert!(
        build_technical_decision_trust_snapshot(&source, &card, 20)
            .await
            .is_err()
    );

    let source = FakeSource::from_card(&card);
    let trust = build_technical_decision_trust_snapshot(&source, &card, 20)
        .await
        .unwrap();
    let mut changed = card.clone();
    let TechnicalFactObservation::Verified { value, .. } = &mut changed.facts[2].observation else {
        unreachable!()
    };
    *value = TechnicalFactValue::Determination(false);
    assert!(
        compare_delivery_mechanisms_with_trust(&changed, "task-1", "2", DIGEST, 20, &trust,)
            .is_err()
    );
}
