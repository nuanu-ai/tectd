use crate::*;
use uuid::Uuid;

fn source() -> KnowledgeSourceRef {
    KnowledgeSourceRef::Snapshot {
        snapshot: KnowledgeSavedSourceSnapshot {
            title: "source".into(),
            uri: "https://example.test/source".into(),
            text: "exact source text".into(),
            observed_at: Some("2026-09-14T00:00:00Z".into()),
            evidence_kind: KnowledgeEvidenceKind::Document,
        },
    }
}

fn security_document(access_scope: KnowledgeAccessScope) -> KnowledgeDocumentDraft {
    KnowledgeDocumentDraft {
        title: "Security boundary".into(),
        canonical_text: "Restricted evidence remains owner-only.".into(),
        knowledge_kind: KnowledgeKind::Security,
        epistemic_state: KnowledgeEpistemicState::Declared,
        target_iris: vec!["urn:asset:one".into()],
        conditions: vec![],
        exceptions: vec![],
        sources: vec![source()],
        bindings: vec![KnowledgeDocumentBinding {
            target: KnowledgeBindingTarget::Workspace,
            purpose: KnowledgeBindingPurpose::Required,
            version_resolution: KnowledgeBindingVersion::CurrentAccepted,
        }],
        profiles: vec![KnowledgeProfileId::General, KnowledgeProfileId::Security],
        access_scope,
        owner_ref: "workspace-owner".into(),
        authority_basis: "authenticated workspace owner".into(),
        graph_assertions: vec![],
        planning_briefs: vec![],
        valid_from: None,
        valid_until: None,
        review_due_at: None,
        sections: KnowledgeProfileSections {
            security: Some(KnowledgeSecuritySection {
                asset_iris: vec!["urn:asset:one".into()],
                trust_boundaries: vec!["service boundary".into()],
                threats: vec!["unauthorized disclosure".into()],
                controls: vec!["owner-only delivery".into()],
                evidence_refs: vec![0],
                verification_status: "documented".into(),
                sensitivity: KnowledgeSensitivity::Restricted,
                applicable_authority: "workspace owner".into(),
                exceptions: vec![],
                finding_state: "open".into(),
                remediation_proof_refs: vec![],
            }),
            ..KnowledgeProfileSections::default()
        },
    }
}

#[test]
fn graph_assertions_round_trip_and_old_documents_stay_unchanged() {
    let mut document = security_document(KnowledgeAccessScope::OwnersOnly);
    let old_json = serde_json::to_value(&document).unwrap();
    assert!(old_json.get("graph_assertions").is_none());
    assert_eq!(
        serde_json::from_value::<KnowledgeDocumentDraft>(old_json.clone()).unwrap(),
        document
    );

    document.graph_assertions = vec![KnowledgeGraphAssertion {
        subject_iri: "urn:concept:child".into(),
        predicate: KnowledgeGraphPredicate::BroaderConcept,
        object_iri: "https://example.test/concepts/parent".into(),
    }];
    assert_eq!(document.validate(), Ok(()));
    let json = serde_json::to_value(&document).unwrap();
    assert_eq!(json["graph_assertions"][0]["predicate"], "broader_concept");
    assert_eq!(
        serde_json::from_value::<KnowledgeDocumentDraft>(json).unwrap(),
        document
    );
}

#[test]
fn graph_assertions_reject_invalid_iris_duplicates_and_excess_count() {
    let base = security_document(KnowledgeAccessScope::OwnersOnly);
    let assertion = KnowledgeGraphAssertion {
        subject_iri: "urn:concept:child".into(),
        predicate: KnowledgeGraphPredicate::ClassifiedAs,
        object_iri: "urn:class:parent".into(),
    };
    let mut document = base.clone();
    document.graph_assertions = vec![assertion.clone(), assertion.clone()];
    assert_eq!(document.validate(), Err(Error::InvalidArguments));

    for bad_iri in [
        "relative/path",
        "urn:has space",
        "https://example.test/has space",
    ] {
        let mut document = base.clone();
        document.graph_assertions = vec![KnowledgeGraphAssertion {
            subject_iri: bad_iri.into(),
            ..assertion.clone()
        }];
        assert_eq!(document.validate(), Err(Error::InvalidArguments));
        document.graph_assertions[0].subject_iri = assertion.subject_iri.clone();
        document.graph_assertions[0].object_iri = bad_iri.into();
        assert_eq!(document.validate(), Err(Error::InvalidArguments));
    }

    let mut document = base;
    document.graph_assertions = (0..=DK2_MAX_LIST_ITEMS)
        .map(|index| KnowledgeGraphAssertion {
            subject_iri: format!("urn:concept:{index}"),
            ..assertion.clone()
        })
        .collect();
    assert_eq!(document.validate(), Err(Error::InvalidArguments));

    let mut json = serde_json::to_value(&document).unwrap();
    json["graph_assertions"][0]["predicate"] = "unexpected".into();
    assert!(serde_json::from_value::<KnowledgeDocumentDraft>(json).is_err());
}

fn begin() -> BeginKnowledgeChange {
    BeginKnowledgeChange {
        request_id: Uuid::new_v4(),
        intent: "Publish one bounded unit".into(),
        desired_outcome: "Canonical exact delivery".into(),
        sources: vec![source()],
        operation_hints: vec![KnowledgeOperationHint {
            client_label: "create-one".into(),
            operation: KnowledgeLifecycleOperation::Create,
            unit_id: None,
            expected_revision: None,
            expected_lifecycle: None,
            reason: "approved source".into(),
            authority_basis: "workspace owner".into(),
            depends_on_labels: vec![],
        }],
        owner: KnowledgeChangeOwner::Workspace,
        completion: KnowledgeCompletionRequirement {
            canonical_result: true,
            exact_delivery: true,
            impact_recorded: true,
            search: KnowledgeSearchRequirement::NotRequired,
            erasure: KnowledgeErasureRequirement::NotRequired,
        },
        delivery_mode: None,
    }
}

#[test]
fn phase_ids_serialize_to_the_exact_contract_ids() {
    let values = KnowledgeChangePhaseId::ALL
        .iter()
        .map(|phase| phase.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        [
            "kc-intake",
            "kc-resolve-baseline",
            "kc-qualify-plan",
            "kc-qualify-evidence",
            "kc-prepare-change",
            "kc-domain-checks",
            "kc-impact-plan",
            "kc-review-reconcile",
            "kc-publication-gate",
            "kc-commit",
            "kc-settle-effects",
            "kc-result-handoff",
        ]
    );
}

#[test]
fn sensitive_security_payload_requires_owner_only_access() {
    assert_eq!(
        security_document(KnowledgeAccessScope::WorkspaceMembers).validate(),
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        security_document(KnowledgeAccessScope::OwnersOnly).validate(),
        Ok(())
    );
}

#[test]
fn sections_timestamps_and_aggregate_capacity_fail_closed() {
    let mut document = security_document(KnowledgeAccessScope::OwnersOnly);
    document.valid_from = Some("2026-09-15T00:00:00Z".into());
    document.valid_until = Some("2026-09-14T00:00:00Z".into());
    assert_eq!(document.validate(), Err(Error::InvalidArguments));

    let mut document = security_document(KnowledgeAccessScope::OwnersOnly);
    document.valid_from = Some("2026-02-29T00:00:00Z".into());
    assert_eq!(document.validate(), Err(Error::InvalidArguments));

    let mut document = security_document(KnowledgeAccessScope::OwnersOnly);
    document.knowledge_kind = KnowledgeKind::Claim;
    document.profiles = vec![KnowledgeProfileId::General];
    document.sections.general = Some(KnowledgeGeneralSection {
        statement: "claim".into(),
        assumptions: vec![],
        evidence_scope: "workspace".into(),
        rationale: String::new(),
        alternatives: vec![],
        negative_limits: vec![],
        unknown_limits: vec![],
    });
    assert_eq!(document.validate(), Err(Error::InvalidArguments));

    let mut document = security_document(KnowledgeAccessScope::OwnersOnly);
    document.canonical_text = "c".repeat(DK2_MAX_SOURCE_BYTES);
    if let KnowledgeSourceRef::Snapshot { snapshot } = &mut document.sources[0] {
        snapshot.text = "s".repeat(DK2_MAX_SOURCE_BYTES);
    }
    assert_eq!(document.validate(), Err(Error::CapacityExceeded));
}

#[test]
fn begin_rejects_too_many_operations() {
    let mut request = begin();
    request.operation_hints = (0..=DK2_MAX_OPERATIONS)
        .map(|index| KnowledgeOperationHint {
            client_label: format!("create-{index}"),
            ..request.operation_hints[0].clone()
        })
        .collect();
    assert_eq!(request.validate(), Err(Error::InvalidArguments));
}

#[test]
fn machine_phases_reject_agent_output_and_commit_has_its_own_route() {
    let base = CompleteKnowledgeChangePhase {
        request_id: Uuid::new_v4(),
        change_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: KnowledgeChangePhaseId::KcPublicationGate,
        output: None,
        revisit_phase_id: None,
    };
    assert_eq!(base.validate(), Ok(()));
    assert_eq!(
        CompleteKnowledgeChangePhase {
            phase_id: KnowledgeChangePhaseId::KcCommit,
            ..base
        }
        .validate(),
        Err(Error::InvalidArguments)
    );
}
