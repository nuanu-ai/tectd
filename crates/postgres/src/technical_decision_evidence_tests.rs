use super::*;

#[test]
fn private_server_approval_configuration_preserves_exact_metadata_and_denies_ambiguity() {
    let (a, _) = fixture(true);
    let b = &a.binding;
    let p = &a.approval;
    let value = serde_json::json!([{
        "binding": {"tenant_id": b.tenant_id,"workspace_id":b.workspace_id,
            "task_id":b.task_id,"task_revision":b.task_revision,
            "operating_verification_digest":b.operating_verification_digest,
            "operating_policy_version":b.operating_policy_version,
            "requirements_binding":{"locator": b.requirements_binding.locator.as_json(),
                "snapshot_id":b.requirements_binding.snapshot_id,"semantic_digest":b.requirements_binding.semantic_digest,
                "authority_schema":b.requirements_binding.authority_schema},
            "choice_set":b.choice_set,"choice_set_digest":b.choice_set_digest,
            "recorded_by_principal_id":b.recorded_by_principal_id},
        "reference":{"artifact_id":a.reference.artifact_id,"artifact_version":a.reference.artifact_version,
            "content_sha256":a.reference.content_sha256},
        "approval":{"claim":p.claim,"card_digest":p.card_digest,"candidate_digest":p.candidate_digest,
            "choice_set_digest":p.choice_set_digest,"recorded_by_principal_id":p.recorded_by_principal_id,
            "owner_author_principal_id":p.owner_author_principal_id,"owner_authorship_ref":p.owner_authorship_ref},
        "candidate_mapping":a.candidate_mapping.iter().map(|m| serde_json::json!({
            "frozen_candidate":m.frozen_candidate,"technical_approach":m.technical_approach})).collect::<Vec<_>>(),
        "validator_policy_version":a.validator_policy_version,"max_age_seconds":a.max_age_seconds
    }]);
    let parsed = parse_technical_decision_approvals(&value.to_string()).unwrap();
    assert_eq!(parsed[0].binding, a.binding);
    assert_eq!(parsed[0].approval, a.approval);
    assert!(parse_technical_decision_approvals("[]").unwrap().is_empty());
    assert!(parse_technical_decision_approvals("[{\"binding\":{},\"binding\":{}}]").is_err());
    for pointer in [
        "/0/approval",
        "/0/reference",
        "/0/binding",
        "/0/binding/requirements_binding/locator",
        "/0/candidate_mapping/0/technical_approach",
    ] {
        let mut bad = value.clone();
        bad.pointer_mut(pointer).unwrap()["forged"] = serde_json::json!(true);
        assert!(parse_technical_decision_approvals(&bad.to_string()).is_err());
    }
    let mut duplicate = value.clone();
    duplicate.as_array_mut().unwrap().push(value[0].clone());
    assert!(parse_technical_decision_approvals(&duplicate.to_string()).is_err());
    let mut forged_owner = value.clone();
    forged_owner[0]["approval"]["owner_author_principal_id"] = serde_json::json!(Uuid::new_v4());
    assert!(parse_technical_decision_approvals(&forged_owner.to_string()).is_err());
    let id = Uuid::new_v4();
    for locator in [
        MatrixRequirementsLocator::Program { program_id: id },
        MatrixRequirementsLocator::Scope {
            program_id: id,
            scope_id: id,
        },
        MatrixRequirementsLocator::Slice {
            program_id: id,
            scope_id: id,
            candidate_set_id: id,
            work_candidate_id: id,
            expected_work_revision: 1,
        },
        MatrixRequirementsLocator::OpenedSlice { slice_id: id },
    ] {
        let mut configured = value.clone();
        configured[0]["binding"]["requirements_binding"]["locator"] = locator.as_json();
        assert_eq!(
            parse_technical_decision_approvals(&configured.to_string()).unwrap()[0]
                .binding
                .requirements_binding
                .locator,
            locator
        );
    }
}
use tect_application::{MatrixRequirementsLocator, MatrixTaskRequirementsBinding};
use tect_domain::{
    DeliveryApproachKind, EngineeringChoiceSet, MATRIX_CHOICE_SET_SCHEMA,
    TechnicalApprovalAuthority, TechnicalOwnerApprovalClaim, TechnicalSourceSupport,
};

fn fixture(separate_required: bool) -> (ApprovedTechnicalDecisionEvidence, String) {
    let task_id = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let candidates: Vec<_> = ["reuse", "separate"]
        .into_iter()
        .map(|id| EngineeringCandidate {
            candidate_id: id.into(),
            title: id.into(),
            approach: format!("{id} mechanism"),
            assumption_fact_ids: vec!["criticality".into()],
        })
        .collect();
    let binding = ServerTechnicalDecisionTaskBinding {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        task_id,
        task_revision: 3,
        operating_verification_digest: "a".repeat(64),
        operating_policy_version: "operating/1".into(),
        requirements_binding: MatrixTaskRequirementsBinding {
            locator: MatrixRequirementsLocator::Program {
                program_id: Uuid::new_v4(),
            },
            snapshot_id: Uuid::new_v4(),
            semantic_digest: "b".repeat(64),
            authority_schema: "requirements/1".into(),
        },
        choice_set: EngineeringChoiceSet {
            schema: MATRIX_CHOICE_SET_SCHEMA.into(),
            choice_set_id: "choices".into(),
            version: 1,
            task_id: task_id.to_string(),
            task_revision: "3".into(),
            decision_question: "Which delivery mechanism?".into(),
            candidates,
        },
        choice_set_digest: "c".repeat(64),
        recorded_by_principal_id: owner,
    };
    let mappings: Vec<_> = binding
        .choice_set
        .candidates
        .iter()
        .enumerate()
        .map(|(index, c)| TechnicalDecisionCandidateMapping {
            frozen_candidate: c.clone(),
            technical_approach: DeliveryApproach {
                id: c.candidate_id.clone(),
                title: c.title.clone(),
                mechanism: c.approach.clone(),
                kind: if index == 0 {
                    DeliveryApproachKind::ReuseExistingPath
                } else {
                    DeliveryApproachKind::SeparateMechanism
                },
                operational_consequences: vec!["Maintain the approved path".into()],
            },
        })
        .collect();
    let kinds = [
        TechnicalFactKind::ReuseSourceSupport,
        TechnicalFactKind::SeparateSourceSupport,
        TechnicalFactKind::ReuseMeetsOutcome,
        TechnicalFactKind::ReuseOperationsAcceptable,
        TechnicalFactKind::SeparateMeetsOutcome,
        TechnicalFactKind::SeparateOperationsAcceptable,
        TechnicalFactKind::SeparateRequiredByConstraint,
    ];
    let artifact = TechnicalDecisionEvidenceArtifact {
        schema: TECHNICAL_EVIDENCE_SCHEMA.into(),
        tenant_id: binding.tenant_id,
        workspace_id: binding.workspace_id,
        task_id,
        task_revision: 3,
        operating_verification_digest: binding.operating_verification_digest.clone(),
        operating_policy_version: binding.operating_policy_version.clone(),
        requirements: TechnicalEvidenceRequirements {
            locator: TechnicalEvidenceLocator::from(&binding.requirements_binding.locator),
            snapshot_id: binding.requirements_binding.snapshot_id,
            semantic_digest: binding.requirements_binding.semantic_digest.clone(),
            authority_schema: binding.requirements_binding.authority_schema.clone(),
        },
        choice_set_digest: binding.choice_set_digest.clone(),
        decision_question: binding.choice_set.decision_question.clone(),
        required_outcome: "Deliver real capability".into(),
        candidate_mapping: mappings
            .iter()
            .map(|m| TechnicalEvidenceCandidateMapping {
                frozen_candidate: m.frozen_candidate.clone(),
                technical_approach: m.technical_approach.clone(),
            })
            .collect(),
        facts: kinds
            .into_iter()
            .enumerate()
            .map(|(i, kind)| TechnicalEvidenceObservation {
                kind,
                value: if i < 2 {
                    TechnicalFactValue::SourceSupport(TechnicalSourceSupport::Supported)
                } else {
                    TechnicalFactValue::Determination(i != 6 || separate_required)
                },
                source_ref: format!("immutable-observation:{i}"),
                observed_at: 900,
                expires_at: 1100,
            })
            .collect(),
    };
    let body = serde_json::to_string(&artifact).unwrap();
    let reference = TechnicalDecisionEvidenceReference {
        artifact_id: Uuid::new_v4(),
        artifact_version: 1,
        content_sha256: format!("{:x}", Sha256::digest(body.as_bytes())),
    };
    let facts = artifact
        .facts
        .iter()
        .map(|o| TechnicalDecisionFact {
            kind: o.kind,
            observation: TechnicalFactObservation::Verified {
                value: o.value,
                binding: TechnicalEvidenceBinding {
                    task_id: task_id.to_string(),
                    task_revision: "3".into(),
                    matrix_verification_digest: binding.operating_verification_digest.clone(),
                    evidence_ref: format!("pipeline-evidence:{}@1", reference.artifact_id),
                    content_digest: reference.content_sha256.clone(),
                    validator_policy_version: TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION.into(),
                    observed_at: o.observed_at,
                    expires_at: o.expires_at,
                    validation_outcome: EvidenceValidationOutcome::Accepted,
                },
            },
        })
        .collect();
    let mut card = DeliveryMechanismDecisionCard {
        schema: TECHNICAL_DECISION_SCHEMA.into(),
        card_id: DELIVERY_MECHANISM_CARD.into(),
        task_id: task_id.to_string(),
        task_revision: "3".into(),
        matrix_verification_digest: binding.operating_verification_digest.clone(),
        decision_question: artifact.decision_question.clone(),
        required_outcome: artifact.required_outcome.clone(),
        approaches: mappings
            .iter()
            .map(|m| m.technical_approach.clone())
            .collect(),
        facts,
        owner_approval: TechnicalOwnerApprovalClaim {
            approval_ref: "technical-approval:1".into(),
            approving_principal: owner.to_string(),
            authority: TechnicalApprovalAuthority::Owner,
            task_id: task_id.to_string(),
            task_revision: "3".into(),
            candidate_digest: String::new(),
            approved_at: 950,
        },
    };
    card.owner_approval.candidate_digest = card.candidate_digest().unwrap();
    let approval = TechnicalDecisionApprovalRecord {
        claim: card.owner_approval.clone(),
        card_digest: card.canonical_digest().unwrap(),
        candidate_digest: card.candidate_digest().unwrap(),
        choice_set_digest: binding.choice_set_digest.clone(),
        recorded_by_principal_id: owner,
        owner_author_principal_id: owner,
        owner_authorship_ref: "owner-authored-source:1".into(),
    };
    (
        ApprovedTechnicalDecisionEvidence {
            binding,
            reference,
            approval,
            candidate_mapping: mappings,
            validator_policy_version: TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION.into(),
            max_age_seconds: 100,
        },
        body,
    )
}
fn row(body: &str) -> ArtifactRow {
    (
        format!("{:x}", Sha256::digest(body.as_bytes())),
        body.len() as i64,
        TECHNICAL_EVIDENCE_FORMAT.into(),
        "ready".into(),
        body.into(),
    )
}
fn resolve(
    a: &ApprovedTechnicalDecisionEvidence,
    body: &str,
    now: i64,
) -> Result<Option<ResolvedTechnicalDecisionEvidence>> {
    checked_technical_snapshot(Some(row(body)), &a.binding, &a.reference, a, now)
}
fn rejected(a: &ApprovedTechnicalDecisionEvidence, body: &str, now: i64) {
    assert!(!matches!(resolve(a, body, now), Ok(Some(_))));
}
fn approved_bytes(a: &mut ApprovedTechnicalDecisionEvidence, value: &serde_json::Value) -> String {
    let body = serde_json::to_string(value).unwrap();
    a.reference.content_sha256 = row(&body).0;
    body
}

include!("technical_decision_evidence_tests/binding_negatives.rs");
