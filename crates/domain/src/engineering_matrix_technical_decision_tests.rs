use super::*;

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn binding() -> TechnicalEvidenceBinding {
    TechnicalEvidenceBinding {
        task_id: "task-1".into(),
        task_revision: "2".into(),
        matrix_verification_digest: DIGEST.into(),
        evidence_ref: "artifact-1@1".into(),
        content_digest: DIGEST.into(),
        validator_policy_version: "source-validator@1".into(),
        observed_at: 10,
        expires_at: 30,
        validation_outcome: EvidenceValidationOutcome::Accepted,
    }
}

fn verified(kind: TechnicalFactKind, value: TechnicalFactValue) -> TechnicalDecisionFact {
    TechnicalDecisionFact {
        kind,
        observation: TechnicalFactObservation::Verified {
            value,
            binding: binding(),
        },
    }
}

fn determination(kind: TechnicalFactKind, value: bool) -> TechnicalDecisionFact {
    verified(kind, TechnicalFactValue::Determination(value))
}

fn card() -> DeliveryMechanismDecisionCard {
    let mut card = DeliveryMechanismDecisionCard {
        schema: TECHNICAL_DECISION_SCHEMA.into(),
        card_id: DELIVERY_MECHANISM_CARD.into(),
        task_id: "task-1".into(),
        task_revision: "2".into(),
        matrix_verification_digest: DIGEST.into(),
        decision_question: "How should updates reach the agreed audience?".into(),
        required_outcome: "Deliver actual requested updates with recovery proof".into(),
        approaches: vec![
            DeliveryApproach {
                id: "owner-option-a".into(),
                kind: DeliveryApproachKind::ReuseExistingPath,
                title: "Existing path".into(),
                mechanism: "Reuse the observed delivery runtime".into(),
                operational_consequences: vec![
                    "Use its current deployment and recovery path".into(),
                ],
            },
            DeliveryApproach {
                id: "owner-option-b".into(),
                kind: DeliveryApproachKind::SeparateMechanism,
                title: "Separate mechanism".into(),
                mechanism: "Introduce another update delivery path".into(),
                operational_consequences: vec!["Operate and recover another path".into()],
            },
        ],
        owner_approval: TechnicalOwnerApprovalClaim {
            approval_ref: "owner-decision-1@2".into(),
            approving_principal: "owner-1".into(),
            authority: TechnicalApprovalAuthority::Owner,
            task_id: "task-1".into(),
            task_revision: "2".into(),
            candidate_digest: String::new(),
            approved_at: 12,
        },
        facts: vec![
            verified(
                TechnicalFactKind::ReuseSourceSupport,
                TechnicalFactValue::SourceSupport(TechnicalSourceSupport::Supported),
            ),
            verified(
                TechnicalFactKind::SeparateSourceSupport,
                TechnicalFactValue::SourceSupport(TechnicalSourceSupport::Supported),
            ),
            determination(TechnicalFactKind::ReuseMeetsOutcome, true),
            determination(TechnicalFactKind::ReuseOperationsAcceptable, true),
            determination(TechnicalFactKind::SeparateMeetsOutcome, true),
            determination(TechnicalFactKind::SeparateOperationsAcceptable, true),
            determination(TechnicalFactKind::SeparateRequiredByConstraint, false),
        ],
    };
    card.owner_approval.candidate_digest = card.candidate_digest().unwrap();
    card
}

/// Test-only stand-in for a server-owned resolver with exact stored records.
struct MockTrust {
    approval: TechnicalOwnerApprovalClaim,
    facts: Vec<TechnicalDecisionFact>,
}

impl MockTrust {
    fn from_card(card: &DeliveryMechanismDecisionCard) -> Self {
        Self {
            approval: card.owner_approval.clone(),
            facts: card.facts.clone(),
        }
    }
}

impl TechnicalDecisionTrust for MockTrust {
    fn validate_fact(
        &self,
        _: &DeliveryMechanismDecisionCard,
        fact: &TechnicalDecisionFact,
        _: &TechnicalEvidenceBinding,
        _: i64,
    ) -> Result<bool> {
        Ok(self.facts.contains(fact))
    }

    fn validate_owner_approval(
        &self,
        card: &DeliveryMechanismDecisionCard,
        candidate_digest: &str,
        _: i64,
    ) -> Result<bool> {
        Ok(card.owner_approval == self.approval
            && candidate_digest == self.approval.candidate_digest)
    }
}

fn compare(card: &DeliveryMechanismDecisionCard) -> Result<DeliveryMechanismComparison> {
    compare_delivery_mechanisms_with_trust(
        card,
        "task-1",
        "2",
        DIGEST,
        20,
        &MockTrust::from_card(card),
    )
}

fn set_bool(card: &mut DeliveryMechanismDecisionCard, kind: TechnicalFactKind, value: bool) {
    let fact = card
        .facts
        .iter_mut()
        .find(|fact| fact.kind == kind)
        .unwrap();
    if let TechnicalFactObservation::Verified { value: found, .. } = &mut fact.observation {
        *found = TechnicalFactValue::Determination(value);
    }
}

#[test]
fn supported_adequate_reuse_dominates_needless_extra_mechanism() {
    let card = card();
    let before = card.clone();
    let result = compare(&card).unwrap();
    assert_eq!(card, before); // Comparison has no disposition or effect mutation.
    assert_eq!(result.eligible_approach_ids, ["owner-option-a"]);
    assert_eq!(
        result.assessments[0].source_support,
        TechnicalSourceSupport::Supported
    );
    assert_eq!(
        result.assessments[0].engineering_adequacy,
        TechnicalAdequacy::Adequate
    );
    assert_eq!(
        result.assessments[1].source_support,
        TechnicalSourceSupport::Supported
    );
    assert_eq!(
        result.assessments[1].engineering_adequacy,
        TechnicalAdequacy::Dominated
    );
}

#[test]
fn unknown_evidence_cannot_make_either_approach_eligible() {
    let mut card = card();
    card.facts[2].observation = TechnicalFactObservation::NeedsInspection {
        reason: "Inspect existing path against requested behavior".into(),
    };
    let result = compare(&card).unwrap();
    assert!(result.eligible_approach_ids.is_empty());
    assert_eq!(result.assessments.len(), 2);
    assert!(result.assessments.iter().all(|assessment| {
        assessment.source_support == TechnicalSourceSupport::Supported
            && assessment.engineering_adequacy == TechnicalAdequacy::Unknown
            && !assessment.eligible
    }));
    assert_eq!(
        result.needs_inspection,
        [TechnicalFactKind::ReuseMeetsOutcome]
    );
}

#[test]
fn same_mode_and_question_with_different_verified_facts_changes_comparison() {
    let baseline = card();
    let mut different = baseline.clone();
    set_bool(&mut different, TechnicalFactKind::ReuseMeetsOutcome, false);
    let first = compare(&baseline).unwrap();
    let second = compare(&different).unwrap();
    assert_eq!(first.eligible_approach_ids, ["owner-option-a"]);
    assert_eq!(second.eligible_approach_ids, ["owner-option-b"]);
    assert_ne!(first.card_digest, second.card_digest);
}

#[test]
fn conflicting_and_stale_evidence_fail_closed() {
    let mut conflict = card();
    conflict.facts[3] = determination(TechnicalFactKind::ReuseMeetsOutcome, false);
    assert!(compare(&conflict).is_err());
    let mut stale = card();
    if let TechnicalFactObservation::Verified { binding, .. } = &mut stale.facts[2].observation {
        binding.expires_at = 20;
    }
    assert!(compare(&stale).is_err());
    let mut wrong_revision = card();
    if let TechnicalFactObservation::Verified { binding, .. } =
        &mut wrong_revision.facts[2].observation
    {
        binding.task_revision = "1".into();
    }
    assert!(compare(&wrong_revision).is_err());
    assert!(compare_delivery_mechanisms(&card(), "task-1", "3", DIGEST, 20).is_err());
}

#[test]
fn harmless_fact_order_does_not_change_card_digest() {
    let original = card();
    let mut reordered = original.clone();
    reordered.facts.reverse();
    reordered.approaches.reverse();
    let first = compare(&original).unwrap();
    let second = compare(&reordered).unwrap();
    assert_eq!(first.card_digest, second.card_digest);
    assert_eq!(first.eligible_approach_ids, second.eligible_approach_ids);
}

#[test]
fn technology_names_are_neither_forbidden_nor_proof() {
    let mut card = card();
    card.approaches[0].mechanism = "Existing PostgreSQL-backed runtime".into();
    card.approaches[1].mechanism = "Argo plus Terraform delivery".into();
    card.owner_approval.candidate_digest = card.candidate_digest().unwrap();
    assert_eq!(
        compare(&card).unwrap().eligible_approach_ids,
        ["owner-option-a"]
    );
    set_bool(
        &mut card,
        TechnicalFactKind::SeparateRequiredByConstraint,
        true,
    );
    assert_eq!(
        compare(&card).unwrap().eligible_approach_ids,
        ["owner-option-b"]
    );
}

#[test]
fn free_text_and_unvalidated_claims_do_not_establish_eligibility() {
    let mut card = card();
    card.approaches[1].mechanism = "Terraform is mandatory".into();
    card.owner_approval.candidate_digest = card.candidate_digest().unwrap();
    assert_eq!(
        compare(&card).unwrap().eligible_approach_ids,
        ["owner-option-a"]
    );
    if let TechnicalFactObservation::Verified { binding, .. } = &mut card.facts[6].observation {
        binding.validation_outcome = EvidenceValidationOutcome::Rejected;
    }
    assert!(compare(&card).is_err());
}

#[test]
fn deserialized_accepted_claims_default_deny_without_trusted_ports() {
    let card = card();
    assert!(compare_delivery_mechanisms(&card, "task-1", "2", DIGEST, 20).is_err());
}

#[test]
fn forged_accepted_fact_and_owner_fields_cannot_reuse_frozen_validation() {
    let original = card();
    let trust = MockTrust::from_card(&original);
    let mut forged_fact = original.clone();
    set_bool(
        &mut forged_fact,
        TechnicalFactKind::ReuseMeetsOutcome,
        false,
    );
    assert!(
        compare_delivery_mechanisms_with_trust(&forged_fact, "task-1", "2", DIGEST, 20, &trust)
            .is_err()
    );

    let mut forged_owner = original.clone();
    forged_owner.owner_approval.approval_ref = "self-asserted-owner-approval".into();
    assert!(
        compare_delivery_mechanisms_with_trust(&forged_owner, "task-1", "2", DIGEST, 20, &trust)
            .is_err()
    );

    let mut changed_candidates = original.clone();
    changed_candidates.approaches[1].mechanism = "Different delivery mechanism".into();
    changed_candidates.owner_approval.candidate_digest =
        changed_candidates.candidate_digest().unwrap();
    assert!(
        compare_delivery_mechanisms_with_trust(
            &changed_candidates,
            "task-1",
            "2",
            DIGEST,
            20,
            &trust
        )
        .is_err()
    );
}
