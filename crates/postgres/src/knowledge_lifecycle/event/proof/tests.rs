use super::*;

fn input() -> rdf::RdfPublicationInput {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../rdf/fixtures/general-constraint.json")).unwrap();
    rdf::RdfPublicationInput {
        tenant: Uuid::from_u128(1),
        workspace: Uuid::from_u128(2),
        change_id: Uuid::from_u128(3),
        event_id: Uuid::from_u128(4),
        content_revision: 1,
        planned: KnowledgePlannedOperation {
            operation_id: Uuid::from_u128(5),
            unit_id: Uuid::from_u128(6),
            client_label: "test".into(),
            operation: KnowledgeLifecycleOperation::Create,
            expected_revision: None,
            expected_lifecycle: None,
            document: Some(decode(fixture["document"].clone()).unwrap()),
            revalidation: None,
            successor: None,
            replacement_bindings: vec![],
            reason: "test".into(),
            authority_basis: "test".into(),
            dependency_operation_ids: vec![],
            binding_pins: vec![],
        },
        principal_id: Uuid::from_u128(7),
        session_id: Uuid::from_u128(8),
        resolved_sources: vec![],
        successor_unit: None,
        include_empty_planning_briefs: false,
    }
}

fn event_row(input: &rdf::RdfPublicationInput) -> PublicationEventRow {
    (
        Some(json(input).unwrap()),
        Some("rdf-digest".into()),
        input.planned.unit_id,
        input.content_revision,
        "create".into(),
        input.change_id,
        input.planned.operation_id,
        false,
    )
}

fn operation(input: &rdf::RdfPublicationInput) -> KnowledgeAppliedOperationReceipt {
    let unit = format!(
        "urn:tect:dk:unit:{}:{}:{}",
        input.tenant, input.workspace, input.planned.unit_id
    );
    KnowledgeAppliedOperationReceipt {
        operation_id: input.planned.operation_id,
        unit_id: input.planned.unit_id,
        operation: input.planned.operation,
        revision: Some(input.content_revision),
        event_id: input.event_id,
        revision_iri: Some(format!("{unit}:revision:{}", input.content_revision)),
        unit_iri: unit,
        event_iri: format!(
            "urn:tect:dk:event:{}:{}:{}",
            input.tenant, input.workspace, input.event_id
        ),
        rdf_digest: "rdf-digest".into(),
        rdf_digest_method: "rdfc-1.0-sha256".into(),
        rdf_digest_scope: KnowledgeRdfDigestScope::RevisionPublicationPayload,
    }
}

fn full_receipt(
    input: &rdf::RdfPublicationInput,
    operation: KnowledgeAppliedOperationReceipt,
) -> PublisherReceiptRow {
    let mut receipt = KnowledgePublisherReceipt {
        id: Uuid::from_u128(9),
        request_id: Uuid::from_u128(10),
        change_id: input.change_id,
        run_id: Uuid::from_u128(11),
        sealed_command_digest: "sealed".into(),
        workspace_generation: 1,
        applied_operations: vec![operation],
        effects: vec![],
        digest: String::new(),
    };
    receipt.digest = digest(&receipt).unwrap();
    (Some(json(&receipt).unwrap()), None)
}

#[test]
fn event_identity_and_erasure_checks_are_shared() {
    let input = input();
    let check = |row| {
        verify_event_payload(
            input.tenant,
            input.workspace,
            input.planned.unit_id,
            input.content_revision,
            input.event_id,
            row,
        )
        .map(|_| ())
    };
    assert_eq!(check(event_row(&input)), Ok(()));
    let mut erased = event_row(&input);
    erased.7 = true;
    assert_eq!(check(erased), Err(Error::KnowledgePayloadErased));
    let mut missing = event_row(&input);
    missing.0 = None;
    assert_eq!(check(missing), Err(Error::InternalInvariant));
    let mut operation = event_row(&input);
    operation.4 = "revalidate".into();
    assert_eq!(check(operation), Err(Error::InternalInvariant));
    for field in ["tenant", "workspace", "change_id", "event_id"] {
        let mut wrong = event_row(&input);
        wrong.0.as_mut().unwrap()[field] = json(&Uuid::from_u128(99)).unwrap();
        assert_eq!(check(wrong), Err(Error::InternalInvariant));
    }
    let mut wrong_revision = event_row(&input);
    wrong_revision.3 += 1;
    assert_eq!(check(wrong_revision), Err(Error::InternalInvariant));
    let mut wrong_unit = event_row(&input);
    wrong_unit.2 = Uuid::from_u128(99);
    assert_eq!(check(wrong_unit), Err(Error::InternalInvariant));
    let mut wrong_operation = event_row(&input);
    wrong_operation.6 = Uuid::from_u128(99);
    assert_eq!(check(wrong_operation), Err(Error::InternalInvariant));
}

#[test]
fn full_and_retained_receipt_checks_are_shared() {
    let input = input();
    let verified = VerifiedPublicationEvent {
        input: input.clone(),
        rdf_digest: "rdf-digest".into(),
    };
    let check = |row| verify_receipt_row(input.tenant, input.workspace, &verified, row);
    assert_eq!(check(full_receipt(&input, operation(&input))), Ok(()));
    let mut tampered = full_receipt(&input, operation(&input));
    tampered.0.as_mut().unwrap()["digest"] = serde_json::json!("tampered");
    assert_eq!(check(tampered), Err(Error::InternalInvariant));
    let mut wrong = operation(&input);
    wrong.event_id = Uuid::from_u128(99);
    assert_eq!(
        check(full_receipt(&input, wrong)),
        Err(Error::InternalInvariant)
    );
    let full = full_receipt(&input, operation(&input));
    assert_eq!(
        check((full.0.clone(), full.0)),
        Err(Error::InternalInvariant)
    );
    let retained = KnowledgeErasedPublisherReceipt {
        id: Uuid::from_u128(9),
        request_id: Uuid::from_u128(10),
        change_id: input.change_id,
        run_id: Uuid::from_u128(11),
        completion: KnowledgeCompletionRequirement {
            canonical_result: true,
            exact_delivery: true,
            impact_recorded: true,
            search: KnowledgeSearchRequirement::NotRequired,
            erasure: KnowledgeErasureRequirement::NotRequired,
        },
        operations: vec![KnowledgeRetainedOperationReceipt::Intact(operation(&input))],
        effects: vec![],
    };
    assert_eq!(check((None, Some(json(&retained).unwrap()))), Ok(()));
    let mut wrong = retained;
    let KnowledgeRetainedOperationReceipt::Intact(operation) = &mut wrong.operations[0] else {
        unreachable!()
    };
    operation.rdf_digest_scope = KnowledgeRdfDigestScope::LifecycleEventPayload;
    assert_eq!(
        check((None, Some(json(&wrong).unwrap()))),
        Err(Error::InternalInvariant)
    );
}

#[test]
fn scope_identity_and_revision_proof_mode_are_distinct() {
    let input = input();
    let mut scope = PublicationProofScope::new(
        input.tenant,
        input.workspace,
        input.principal_id,
        input.session_id,
    );
    assert!(
        scope.eager_preload(),
        "shared mutation scopes stay eager by default"
    );
    assert_eq!(
        scope.require_identity(
            input.tenant,
            input.workspace,
            input.principal_id,
            input.session_id
        ),
        Ok(())
    );
    assert_eq!(
        scope.require_identity(
            input.tenant,
            input.workspace,
            input.principal_id,
            Uuid::from_u128(99)
        ),
        Err(Error::InternalInvariant)
    );
    let key = PublicationProofKey {
        unit_id: input.planned.unit_id,
        revision: 1,
        event_id: input.event_id,
        include_revision: false,
    };
    scope.expected.insert(
        key,
        ExpectedPublicationMaterial {
            unit_iri: "unit".into(),
            revision_iri: "revision".into(),
            document_json: None,
        },
    );
    assert!(matches!(
        scope.expected_material(
            input.tenant,
            input.workspace,
            input.principal_id,
            input.session_id,
            key
        ),
        Err(Error::InternalInvariant)
    ));
    scope.verified.insert(
        key,
        Arc::new(VerifiedPublicationEvent {
            input: input.clone(),
            rdf_digest: "rdf-digest".into(),
        }),
    );
    assert!(
        scope
            .expected_material(
                input.tenant,
                input.workspace,
                input.principal_id,
                input.session_id,
                key
            )
            .is_ok()
    );
    for (tenant, workspace, principal, session) in [
        (
            Uuid::from_u128(99),
            input.workspace,
            input.principal_id,
            input.session_id,
        ),
        (
            input.tenant,
            Uuid::from_u128(99),
            input.principal_id,
            input.session_id,
        ),
        (
            input.tenant,
            input.workspace,
            Uuid::from_u128(99),
            input.session_id,
        ),
        (
            input.tenant,
            input.workspace,
            input.principal_id,
            Uuid::from_u128(99),
        ),
    ] {
        assert!(matches!(
            scope.expected_material(tenant, workspace, principal, session, key),
            Err(Error::InternalInvariant)
        ));
    }
    assert!(matches!(
        scope.expected_material(
            input.tenant,
            input.workspace,
            input.principal_id,
            input.session_id,
            PublicationProofKey {
                include_revision: true,
                ..key
            }
        ),
        Err(Error::InternalInvariant)
    ));
    assert!(!scope.verified.contains_key(&PublicationProofKey {
        include_revision: true,
        ..key
    }));
}
