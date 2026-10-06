use super::*;
use crate::{
    TechnicalDecisionApprovalRecord, TechnicalDecisionCandidateMapping,
    TechnicalDecisionEvidenceResolver,
};
use tect_domain::{
    EngineeringCandidate, EngineeringChoiceSet, TechnicalFactKind, TechnicalFactValue,
    TechnicalSourceSupport,
};

fn fixture() -> ResolvedTechnicalDecisionEvidence {
    let task_id = Uuid::new_v4();
    let principal = Uuid::new_v4();
    let mut card = crate::technical_decision_trust::tests::card();
    card.task_id = task_id.to_string();
    card.owner_approval.task_id = task_id.to_string();
    for fact in &mut card.facts {
        if let TechnicalFactObservation::Verified { binding, .. } = &mut fact.observation {
            binding.task_id = task_id.to_string();
            binding.validator_policy_version =
                crate::TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION.into();
        }
    }
    card.owner_approval.candidate_digest = card.candidate_digest().unwrap();
    let candidates = card
        .approaches
        .iter()
        .map(|approach| EngineeringCandidate {
            candidate_id: approach.id.clone(),
            title: approach.title.clone(),
            approach: approach.mechanism.clone(),
            assumption_fact_ids: vec!["mode".into()],
        })
        .collect::<Vec<_>>();
    let choice_set = EngineeringChoiceSet {
        schema: tect_domain::MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "choices".into(),
        version: 1,
        task_id: task_id.to_string(),
        task_revision: card.task_revision.clone(),
        decision_question: card.decision_question.clone(),
        candidates: candidates.clone(),
    };
    let binding = ServerTechnicalDecisionTaskBinding {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        task_id,
        task_revision: 2,
        operating_verification_digest: card.matrix_verification_digest.clone(),
        operating_policy_version: "operating@1".into(),
        requirements_binding: MatrixTaskRequirementsBinding {
            locator: MatrixRequirementsLocator::Program {
                program_id: Uuid::new_v4(),
            },
            snapshot_id: Uuid::new_v4(),
            semantic_digest: "b".repeat(64),
            authority_schema: "context@1".into(),
        },
        choice_set,
        choice_set_digest: "c".repeat(64),
        recorded_by_principal_id: principal,
    };
    ResolvedTechnicalDecisionEvidence {
        binding,
        reference: TechnicalDecisionEvidenceReference {
            artifact_id: Uuid::new_v4(),
            artifact_version: 1,
            content_sha256: "d".repeat(64),
        },
        facts: card.facts.clone(),
        approval: TechnicalDecisionApprovalRecord {
            claim: card.owner_approval.clone(),
            card_digest: card.canonical_digest().unwrap(),
            candidate_digest: card.candidate_digest().unwrap(),
            choice_set_digest: "c".repeat(64),
            recorded_by_principal_id: principal,
            owner_author_principal_id: principal,
            owner_authorship_ref: "independently-authenticated-owner-authorship@1".into(),
        },
        candidate_mapping: candidates
            .into_iter()
            .zip(card.approaches.iter().cloned())
            .map(
                |(frozen_candidate, technical_approach)| TechnicalDecisionCandidateMapping {
                    frozen_candidate,
                    technical_approach,
                },
            )
            .collect(),
        card,
        validator_policy_version: crate::TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION.into(),
        max_age_seconds: 15,
    }
}

fn change_fact(
    resolved: &mut ResolvedTechnicalDecisionEvidence,
    kind: TechnicalFactKind,
    value: TechnicalFactValue,
) {
    for fact in &mut resolved.facts {
        if fact.kind == kind {
            let TechnicalFactObservation::Verified { value: saved, .. } = &mut fact.observation
            else {
                unreachable!()
            };
            *saved = value;
        }
    }
    resolved.card.facts = resolved.facts.clone();
    resolved.approval.card_digest = resolved.card.canonical_digest().unwrap();
}

#[tokio::test]
async fn technical_decision_compares_adequacy_independently_from_support() {
    let mut resolved = fixture();
    let TechnicalDeliveryMechanismRead::Compared(result) =
        compare_resolved(&resolved.binding, &resolved.reference, &resolved, 20).await
    else {
        panic!("valid source must compare")
    };
    assert_eq!(result.eligible_approach_ids, ["reuse"]);
    change_fact(
        &mut resolved,
        TechnicalFactKind::SeparateRequiredByConstraint,
        TechnicalFactValue::Determination(true),
    );
    let TechnicalDeliveryMechanismRead::Compared(result) =
        compare_resolved(&resolved.binding, &resolved.reference, &resolved, 20).await
    else {
        panic!()
    };
    assert_eq!(result.eligible_approach_ids, ["separate"]);
    assert_eq!(
        result.assessments[0].source_support,
        TechnicalSourceSupport::Supported
    );
    assert_eq!(
        result.assessments[0].engineering_adequacy,
        tect_domain::TechnicalAdequacy::ViolatesConstraint
    );
    change_fact(
        &mut resolved,
        TechnicalFactKind::SeparateSourceSupport,
        TechnicalFactValue::SourceSupport(TechnicalSourceSupport::Unknown),
    );
    let TechnicalDeliveryMechanismRead::Compared(result) =
        compare_resolved(&resolved.binding, &resolved.reference, &resolved, 20).await
    else {
        panic!()
    };
    assert!(result.eligible_approach_ids.is_empty());
    assert_eq!(
        result.needs_inspection,
        [TechnicalFactKind::SeparateSourceSupport]
    );
}

#[tokio::test]
async fn technical_decision_delegated_approval_preserves_owner_candidate_authorship() {
    let mut resolved = fixture();
    let owner = resolved.binding.recorded_by_principal_id;
    let delegated_approver = Uuid::new_v4();
    assert_ne!(delegated_approver, owner);
    resolved.card.owner_approval.authority =
        tect_domain::TechnicalApprovalAuthority::OwnerDelegated;
    resolved.card.owner_approval.approving_principal = delegated_approver.to_string();
    resolved.approval.claim = resolved.card.owner_approval.clone();
    resolved.approval.card_digest = resolved.card.canonical_digest().unwrap();
    // The delegated technical approver does not replace the independently
    // established Owner adoption/authorship of the immutable task alternatives.
    assert_eq!(resolved.approval.owner_author_principal_id, owner);
    let TechnicalDeliveryMechanismRead::Compared(comparison) =
        compare_resolved(&resolved.binding, &resolved.reference, &resolved, 20).await
    else {
        panic!("matching delegated technical approval must compare")
    };
    assert_eq!(comparison.eligible_approach_ids, ["reuse"]);

    resolved.approval.owner_author_principal_id = delegated_approver;
    assert_eq!(
        compare_resolved(&resolved.binding, &resolved.reference, &resolved, 20).await,
        TechnicalDeliveryMechanismRead::Unavailable
    );
}

#[tokio::test]
async fn technical_decision_rejects_binding_mapping_and_authority_forgery() {
    let saved = fixture();
    type EvidenceMutation = Box<dyn Fn(&mut ResolvedTechnicalDecisionEvidence)>;
    let mutations: Vec<EvidenceMutation> = vec![
        Box::new(|r| r.binding.tenant_id = Uuid::new_v4()),
        Box::new(|r| r.binding.workspace_id = Uuid::new_v4()),
        Box::new(|r| r.binding.task_revision += 1),
        Box::new(|r| r.binding.requirements_binding.semantic_digest = "e".repeat(64)),
        Box::new(|r| r.binding.operating_verification_digest = "e".repeat(64)),
        Box::new(|r| r.reference.artifact_version += 1),
        Box::new(|r| r.approval.card_digest = "e".repeat(64)),
        Box::new(|r| r.approval.candidate_digest = "e".repeat(64)),
        Box::new(|r| r.approval.choice_set_digest = "e".repeat(64)),
        Box::new(|r| r.approval.recorded_by_principal_id = Uuid::new_v4()),
        Box::new(|r| r.approval.owner_author_principal_id = Uuid::new_v4()),
        Box::new(|r| r.approval.owner_authorship_ref.clear()),
        Box::new(|r| {
            r.approval.claim.authority = tect_domain::TechnicalApprovalAuthority::OwnerDelegated
        }),
        Box::new(|r| {
            r.candidate_mapping[0]
                .frozen_candidate
                .assumption_fact_ids
                .clear()
        }),
        Box::new(|r| r.candidate_mapping[1] = r.candidate_mapping[0].clone()),
        Box::new(|r| r.validator_policy_version = "other-policy".into()),
        Box::new(|r| {
            r.facts.pop();
        }),
        Box::new(|r| r.facts[1] = r.facts[0].clone()),
        Box::new(|r| r.card.required_outcome = "forged outcome".into()),
        Box::new(|r| r.max_age_seconds = 9),
    ];
    for mutate in mutations {
        let mut changed = saved.clone();
        mutate(&mut changed);
        assert_eq!(
            compare_resolved(&saved.binding, &saved.reference, &changed, 20).await,
            TechnicalDeliveryMechanismRead::Unavailable
        );
    }
    assert_eq!(
        compare_resolved(&saved.binding, &saved.reference, &saved, 9).await,
        TechnicalDeliveryMechanismRead::Unavailable
    );
    assert_eq!(
        compare_resolved(&saved.binding, &saved.reference, &saved, 30).await,
        TechnicalDeliveryMechanismRead::Unavailable
    );
    let disabled = crate::DisabledTechnicalDecisionEvidenceResolver;
    assert!(
        disabled
            .resolve(&saved.binding, &saved.reference, 20)
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn technical_decision_request_contains_pins_only_and_rejects_bad_pins() {
    let saved = fixture();
    let mut request = CompareTechnicalDeliveryMechanisms {
        task_id: saved.binding.task_id,
        expected_task_revision: 2,
        operating_verification_digest: saved.binding.operating_verification_digest,
        evidence_reference: saved.reference,
    };
    assert!(validate_request(&request).is_ok());
    request.evidence_reference.content_sha256 = "Accepted".into();
    assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
}

struct LockedTaskStore {
    revision: MatrixTaskRevision,
    current: MatrixTaskSource,
    calls: Vec<&'static str>,
}

#[async_trait::async_trait]
impl crate::MatrixTaskStore for LockedTaskStore {
    async fn record_matrix_task(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: Uuid,
        _: &crate::RecordMatrixTask,
        _: &serde_json::Value,
        _: &str,
    ) -> Result<MatrixTaskRevision> {
        panic!("comparison must not write task records")
    }
    async fn matrix_task(&mut self, _: Uuid, _: Uuid) -> Result<Option<MatrixTaskRevision>> {
        panic!("comparison must use locking task read")
    }
    async fn lock_matrix_task(&mut self, _: Uuid, _: Uuid) -> Result<Option<MatrixTaskRevision>> {
        self.calls.push("lock_task");
        Ok(Some(self.revision.clone()))
    }
    async fn matrix_task_source(&mut self, _: Uuid, _: Uuid) -> Result<Option<MatrixTaskSource>> {
        self.calls.push("reread_source");
        Ok(Some(self.current.clone()))
    }
}

#[tokio::test]
async fn technical_decision_locks_then_rereads_and_denies_task_or_binding_drift() {
    let saved = fixture();
    let source = MatrixTaskSource {
        revision: super::super::tests::stored_revision(),
        requirements_binding: Some(saved.binding.requirements_binding.clone()),
    };
    let request = CompareTechnicalDeliveryMechanisms {
        task_id: source.revision.task_id,
        expected_task_revision: source.revision.revision,
        operating_verification_digest: saved.binding.operating_verification_digest,
        evidence_reference: saved.reference,
    };
    let mut store = LockedTaskStore {
        revision: source.revision.clone(),
        current: source.clone(),
        calls: vec![],
    };
    assert!(
        lock_current_source(&mut store, saved.binding.workspace_id, &source, &request)
            .await
            .is_ok()
    );
    assert_eq!(store.calls, ["lock_task", "reread_source"]);
    store.revision.revision += 1;
    assert_eq!(
        lock_current_source(&mut store, saved.binding.workspace_id, &source, &request).await,
        Err(Error::StaleRevision)
    );
    store.revision = source.revision.clone();
    store
        .current
        .requirements_binding
        .as_mut()
        .unwrap()
        .semantic_digest = "e".repeat(64);
    assert_eq!(
        lock_current_source(&mut store, saved.binding.workspace_id, &source, &request).await,
        Err(Error::StaleRevision)
    );
}

#[tokio::test]
async fn exact_owner_adoption_provenance_allows_distinct_delegated_technical_approval() {
    // Synthetic resolved metadata exercises the application gate only. This
    // does not prove a public save, fresh human consent, or a concrete resolver's
    // authentication of adoption, and makes no claim about who typed the prose.
    let mut resolved = fixture();
    let owner = resolved.binding.recorded_by_principal_id;
    let delegated_approver = Uuid::new_v4();
    assert_ne!(owner, delegated_approver);
    resolved.card.owner_approval.authority =
        tect_domain::TechnicalApprovalAuthority::OwnerDelegated;
    resolved.card.owner_approval.approving_principal = delegated_approver.to_string();
    resolved.approval.claim = resolved.card.owner_approval.clone();
    resolved.approval.card_digest = resolved.card.canonical_digest().unwrap();
    resolved.approval.owner_authorship_ref = format!(
        "synthetic-owner-adoption:task={}@{};card={};candidates={};choice={};artifact={}@{}:{}",
        resolved.binding.task_id,
        resolved.binding.task_revision,
        resolved.approval.card_digest,
        resolved.approval.candidate_digest,
        resolved.approval.choice_set_digest,
        resolved.reference.artifact_id,
        resolved.reference.artifact_version,
        resolved.reference.content_sha256,
    );
    assert_eq!(resolved.approval.recorded_by_principal_id, owner);
    assert_eq!(resolved.approval.owner_author_principal_id, owner);
    assert_eq!(
        resolved.approval.choice_set_digest,
        resolved.binding.choice_set_digest
    );
    assert_eq!(
        resolved.approval.candidate_digest,
        resolved.card.candidate_digest().unwrap()
    );
    let TechnicalDeliveryMechanismRead::Compared(comparison) =
        compare_resolved(&resolved.binding, &resolved.reference, &resolved, 20).await
    else {
        panic!("exact Owner adoption metadata must permit trusted comparison")
    };
    assert_eq!(comparison.eligible_approach_ids, ["reuse"]);
    resolved.approval.owner_author_principal_id = delegated_approver;
    assert_eq!(
        compare_resolved(&resolved.binding, &resolved.reference, &resolved, 20).await,
        TechnicalDeliveryMechanismRead::Unavailable
    );
}
