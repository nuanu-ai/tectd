use super::*;

pub(super) fn fixture() -> (
    PagedPipelineKnowledgeResourcePin,
    crate::knowledge_lifecycle::VerifiedPublicationEvent,
) {
    let document: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let unit = Uuid::new_v4();
    let event = Uuid::new_v4();
    let binding = Uuid::new_v4();
    let input = decode(serde_json::json!({
        "tenant": tenant, "workspace": workspace, "change_id": Uuid::new_v4(),
        "event_id": event, "content_revision": 1,
        "planned": {"operation_id":Uuid::new_v4(),"unit_id":unit,"client_label":"fixture",
            "operation":"create","document":document["document"],"replacement_bindings":[],
            "reason":"fixture","authority_basis":"fixture","dependency_operation_ids":[]},
        "principal_id":Uuid::new_v4(),"session_id":Uuid::new_v4(),
        "resolved_sources":[{"pin":{"source_index":0,"digest":"c".repeat(64),
            "evidence_kind":"declaration","observed_at":null,
            "evidence_scope":"workspace","source_iri":"urn:tect:dk:source:fixture"},
            "title":"Pinned source","uri":"urn:test:source","text":"source bytes"}],
        "successor_unit":null
    }))
    .unwrap();
    let verified = crate::knowledge_lifecycle::VerifiedPublicationEvent {
        original_document: None,
        refs: crate::knowledge_lifecycle::rdf::build(&input).unwrap().refs,
        input,
        rdf_digest: "a".repeat(64),
    };
    let pin = decode(serde_json::json!({
        "ordinal":0,"entry_kind":"dk2_event","unit_id":unit,"revision":1,
        "publication_event_id":event,"rdf_digest":"a".repeat(64),"binding_id":binding,
        "binding_pin":{"binding_iri":format!("urn:tect:dk:binding:{binding}"),
            "target":{"kind":"workspace"},"purpose":"required",
            "version_resolution":{"kind":"current_accepted"}},
        "lifecycle":"active","access_scope":"workspace_members",
        "validation_event_id":null,"validation_event_digest":null,
        "projection":{"policy":"full_resources","inquiry_briefs":[]},
        "resource_digest":"b".repeat(64),"resource_bytes":1
    }))
    .unwrap();
    (pin, verified)
}

#[test]
fn exact_immutable_payload_and_commitment() {
    let (mut pin, verified) = fixture();
    let resource = assemble(&pin, verified.clone(), None).unwrap();
    assert_eq!(
        resource.canonical_text,
        verified
            .input
            .planned
            .document
            .as_ref()
            .unwrap()
            .canonical_text
    );
    assert_eq!(resource.binding, pin.binding_pin);
    let bytes = serde_json::to_vec(&resource).unwrap();
    pin.resource_bytes = bytes.len() as i64;
    pin.resource_digest = sha256(&bytes);
    let (got, serialized) = reconstruct(&pin, verified.clone(), None).unwrap();
    assert_eq!(got, resource);
    assert_eq!(serialized, bytes);
    let mut bad = pin.clone();
    bad.resource_digest = "0".repeat(64);
    assert!(matches!(
        reconstruct(&bad, verified.clone(), None),
        Err(Error::InternalInvariant)
    ));
    let mut newer = verified.clone();
    newer.input.content_revision = 2;
    assert!(matches!(
        reconstruct(&pin, newer, None),
        Err(Error::InternalInvariant)
    ));
    let mut another_event = verified;
    another_event.input.event_id = Uuid::new_v4();
    assert!(matches!(
        reconstruct(&pin, another_event, None),
        Err(Error::InternalInvariant)
    ));
}

#[test]
fn dk1_pin_reconstructs_exact_generic_resource_and_fragments() {
    let (mut pin, _) = fixture();
    pin.entry_kind = PagedKnowledgeEntryKind::Dk1Legacy;
    pin.publication_event_id = None;
    let unit = pin.unit_id;
    let revision = KnowledgeUnitRevision {
        unit_id: unit,
        revision: 1,
        active: true,
        constraint: KnowledgeConstraintDraft {
            title: "Legacy title".into(),
            statement: "Pinned legacy statement".repeat(128),
            modality: KnowledgeModality::Must,
            action: "retain".into(),
            target_iri: "urn:target:legacy".into(),
            conditions: vec!["condition".into()],
            exceptions: vec![],
            source: KnowledgeSourceSnapshot {
                title: "Source".into(),
                uri: "urn:source:legacy".into(),
                text: "source text".into(),
            },
            binding: KnowledgeBinding::Workspace,
            purpose: KnowledgePurpose::ExecutionConstraint,
            version_resolution: KnowledgeVersionResolution::CurrentAccepted,
        },
        source_sha256: sha256(b"source text"),
        rdf_digest: pin.rdf_digest.clone(),
        rdf_digest_method: "rdfc-1.0-sha256".into(),
        rdf_digest_scope: KnowledgeRdfDigestScope::RevisionPublicationPayload,
        publication_event_id: Uuid::new_v4(),
        unit_iri: format!("urn:tect:dk:unit:{unit}"),
        revision_iri: format!("urn:tect:dk:revision:{unit}:1"),
        source_iri: "urn:tect:dk:source:legacy".into(),
        publication_event_iri: "urn:tect:dk:event:legacy".into(),
        publication_operation: KnowledgeOperation::Create,
        publication_reason: "test".into(),
        publication_authority_basis: "test".into(),
        publication_actor_principal_id: Uuid::new_v4(),
        publication_actor_session_id: Uuid::new_v4(),
        binding_provenance: None,
    };
    let resource = generic::resource::legacy_from_revision(
        pin.lifecycle,
        revision.clone(),
        pin.access_scope,
        pin.binding_pin.clone(),
    );
    assert_eq!(
        resource.canonical_text,
        "Pinned legacy statement".repeat(128)
    );
    assert_eq!(resource.revision, pin.revision);
    let bytes = serde_json::to_vec(&resource).unwrap();
    pin.resource_bytes = bytes.len() as i64;
    pin.resource_digest = sha256(&bytes);
    let (read_resource, read_bytes) = reconstruct_legacy(&pin, revision.clone()).unwrap();
    assert_eq!(read_resource, resource);
    assert_eq!(read_bytes, bytes);
    let mut page = test_page(Uuid::new_v4());
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let principal = Uuid::new_v4();
    assert_eq!(
        pack_resource(
            &mut page,
            pin.clone(),
            resource.clone(),
            &bytes,
            0,
            0,
            2,
            2048,
            tenant,
            workspace,
            principal,
        )
        .unwrap(),
        PackOutcome::Fragment
    );
    assert_eq!(page.fragment.as_ref().unwrap().sha256, pin.resource_digest);
    assert!(page.next_byte_offset > 0);
    let mut bad = pin.clone();
    bad.resource_digest = "0".repeat(64);
    assert!(matches!(
        reconstruct_legacy(&bad, revision.clone()),
        Err(Error::InternalInvariant)
    ));
    bad = pin.clone();
    bad.revision += 1;
    assert!(matches!(
        reconstruct_legacy(&bad, revision.clone()),
        Err(Error::InternalInvariant)
    ));
    bad = pin;
    bad.rdf_digest = "0".repeat(64);
    assert!(matches!(
        reconstruct_legacy(&bad, revision),
        Err(Error::InternalInvariant)
    ));
}
