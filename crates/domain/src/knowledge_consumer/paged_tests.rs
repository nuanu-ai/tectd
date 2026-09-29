use super::*;
use crate::{KnowledgeBindingTarget, KnowledgeBindingVersion};

fn digest() -> String {
    "a".repeat(64)
}

fn manifest() -> PagedPipelineKnowledgeManifest {
    PagedPipelineKnowledgeManifest {
        contract_version: PAGED_KNOWLEDGE_CONTRACT_VERSION.into(),
        id: Uuid::new_v4(),
        digest: digest(),
        semantic_digest: digest(),
        workspace_generation: 2,
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: "phase".into(),
        definition_version: "v1".into(),
        definition_digest: digest(),
        method_requirements: vec![],
        inquiry: None,
        projection_policy: None,
        unresolved_needs: vec![],
        freshness_warnings: vec![],
        resource_count: 1,
        total_resource_bytes: 17,
        resource_digest_algorithm: PAGED_RESOURCE_DIGEST_ALGORITHM.into(),
        page_route: "slice.pipeline.knowledge_page".into(),
    }
}

fn row() -> PagedPipelineKnowledgeResourcePin {
    PagedPipelineKnowledgeResourcePin {
        ordinal: 0,
        entry_kind: PagedKnowledgeEntryKind::Dk2Event,
        unit_id: Uuid::new_v4(),
        revision: 1,
        publication_event_id: Some(Uuid::new_v4()),
        rdf_digest: digest(),
        binding_id: Uuid::new_v4(),
        binding_pin: PipelineKnowledgeBindingPin {
            binding_iri: "urn:binding:one".into(),
            target: KnowledgeBindingTarget::Workspace,
            purpose: KnowledgeBindingPurpose::Required,
            version_resolution: KnowledgeBindingVersion::CurrentAccepted,
            definition_kind: None,
            definition_version: None,
            definition_digest: None,
        },
        lifecycle: KnowledgeLifecycleState::Active,
        access_scope: KnowledgeAccessScope::WorkspaceMembers,
        validation_event_id: None,
        validation_event_digest: None,
        projection: PagedKnowledgeProjectionPin {
            policy: PipelineKnowledgeProjectionPolicy::FullResources,
            inquiry_briefs: vec![],
        },
        resource_digest: digest(),
        resource_bytes: 17,
    }
}

#[test]
fn paged_digest_commits_header_and_every_ordered_row() {
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let mut header = manifest();
    let first = row();
    let mut second = row();
    second.ordinal = 1;
    header.resource_count = 2;
    header.total_resource_bytes = 34;
    let rows = vec![first, second];
    let expected = paged_manifest_digest(tenant, workspace, &header, &rows, &[], &[]).unwrap();
    assert_eq!(expected.len(), 64);
    header.digest = expected.clone();
    assert_eq!(
        paged_manifest_digest(tenant, workspace, &header, &rows, &[], &[]).unwrap(),
        expected
    );

    let mut tampered = rows.clone();
    tampered[1].resource_digest = "b".repeat(64);
    assert_ne!(
        paged_manifest_digest(tenant, workspace, &header, &tampered, &[], &[]).unwrap(),
        expected
    );
    tampered = rows.clone();
    tampered[1].binding_pin.purpose = KnowledgeBindingPurpose::Reference;
    assert_ne!(
        paged_manifest_digest(tenant, workspace, &header, &tampered, &[], &[]).unwrap(),
        expected
    );
    tampered = rows.clone();
    tampered[1].projection.policy = PipelineKnowledgeProjectionPolicy::ScopePlanningBriefs;
    assert_ne!(
        paged_manifest_digest(tenant, workspace, &header, &tampered, &[], &[]).unwrap(),
        expected
    );
    tampered = rows.clone();
    tampered.swap(0, 1);
    assert_eq!(
        paged_manifest_digest(tenant, workspace, &header, &tampered, &[], &[]),
        Err(crate::Error::InternalInvariant)
    );
    let mut changed_header = header.clone();
    changed_header.definition_version.push('2');
    assert_ne!(
        paged_manifest_digest(tenant, workspace, &changed_header, &rows, &[], &[]).unwrap(),
        expected
    );
}

#[test]
fn mixed_digest_commits_complete_inline_dk1_and_requires_exact_pin_set() {
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let mut header = manifest();
    let mut pin = row();
    pin.entry_kind = PagedKnowledgeEntryKind::Dk1Legacy;
    pin.publication_event_id = None;
    let item = crate::PipelineKnowledgeItem {
        unit_id: pin.unit_id,
        revision: pin.revision,
        rdf_digest: pin.rdf_digest.clone(),
        source_sha256: digest(),
        why_included: "workspace_binding".into(),
        unit_iri: "urn:test:unit".into(),
        revision_iri: "urn:test:revision".into(),
        source_iri: "urn:test:source".into(),
        source_uri: "urn:test:source-uri".into(),
        title: "Original title".into(),
        statement: "Original statement".into(),
        modality: crate::KnowledgeModality::Must,
        action: "retain".into(),
        target_iri: "urn:test:target".into(),
        conditions: vec![],
        exceptions: vec![],
    };
    let expected = paged_manifest_digest(
        tenant,
        workspace,
        &header,
        &[pin.clone()],
        std::slice::from_ref(&item),
        &[],
    )
    .unwrap();
    header.digest = expected.clone();
    let mut changed = item.clone();
    changed.title.push_str(" changed");
    assert_ne!(
        paged_manifest_digest(tenant, workspace, &header, &[pin.clone()], &[changed], &[]).unwrap(),
        expected
    );
    assert_ne!(
        paged_manifest_digest(
            tenant,
            workspace,
            &header,
            &[pin.clone()],
            std::slice::from_ref(&item),
            &["new gap".into()]
        )
        .unwrap(),
        expected
    );
    assert_eq!(
        paged_manifest_digest(tenant, workspace, &header, &[pin.clone()], &[], &[]),
        Err(crate::Error::InternalInvariant)
    );
    assert_eq!(
        paged_manifest_digest(
            tenant,
            workspace,
            &header,
            &[pin.clone()],
            &[item.clone(), item.clone()],
            &[]
        ),
        Err(crate::Error::InternalInvariant)
    );
    let mut extra = item.clone();
    extra.unit_id = Uuid::new_v4();
    assert_eq!(
        paged_manifest_digest(tenant, workspace, &header, &[pin], &[item, extra], &[]),
        Err(crate::Error::InternalInvariant)
    );
}

#[test]
fn compact_manifest_validates_order_and_mixed_kinds() {
    let mut header = manifest();
    let first = row();
    let mut second = row();
    second.ordinal = 1;
    second.entry_kind = PagedKnowledgeEntryKind::Dk1Legacy;
    second.publication_event_id = None;
    header.resource_count = 2;
    header.total_resource_bytes = 34;
    assert_eq!(header.validate(&[first.clone(), second.clone()]), Ok(()));
    second.ordinal = 2;
    assert_eq!(
        header.validate(&[first, second]),
        Err(crate::Error::InternalInvariant)
    );
}

#[test]
fn rejects_missing_proof_digest_mismatch_and_body_field() {
    let header = manifest();
    let mut pin = row();
    assert_eq!(header.validate(&[pin.clone()]), Ok(()));
    pin.publication_event_id = None;
    assert_eq!(
        header.validate(&[pin.clone()]),
        Err(crate::Error::InternalInvariant)
    );
    pin.publication_event_id = Some(Uuid::new_v4());
    pin.resource_digest = "bad".into();
    assert_eq!(
        header.validate(&[pin]),
        Err(crate::Error::InternalInvariant)
    );
    let mut serialized = serde_json::to_value(row()).unwrap();
    serialized["canonical_text"] = serde_json::json!("must stay in source revision");
    assert!(serde_json::from_value::<PagedPipelineKnowledgeResourcePin>(serialized).is_err());
}

#[test]
fn dispatches_legacy_and_paged_without_rewriting_legacy() {
    let paged = serde_json::to_value(manifest()).unwrap();
    assert!(matches!(
        decode_pipeline_resource_manifest(paged).unwrap(),
        PipelineResourceManifestVersion::Paged(_)
    ));
    let legacy = PipelineKnowledgeResourceManifest {
        id: Uuid::new_v4(),
        digest: digest(),
        semantic_digest: digest(),
        workspace_generation: 1,
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: "phase".into(),
        definition_version: "v1".into(),
        definition_digest: digest(),
        method_requirements: vec![],
        inquiry: None,
        projection_policy: None,
        selected: vec![],
        unresolved_needs: vec![],
        freshness_warnings: vec![],
    };
    let value = serde_json::to_value(&legacy).unwrap();
    let decoded = decode_pipeline_resource_manifest(value.clone()).unwrap();
    assert!(matches!(
        decoded,
        PipelineResourceManifestVersion::InlineDk2(_)
    ));
    assert_eq!(serde_json::to_value(&legacy).unwrap(), value);
    let mut unknown = value;
    unknown["contract_version"] = serde_json::json!("dk-3");
    assert_eq!(
        decode_pipeline_resource_manifest(unknown),
        Err(crate::Error::InternalInvariant)
    );
}
