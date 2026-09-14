use super::model::{TypedTerm, TypedTriple, V2};
use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tect_domain::*;
use uuid::Uuid;

mod planning;

#[derive(Deserialize)]
struct CorpusFixture {
    id: String,
    source_path: String,
    source_revision: String,
    source_sha256: String,
    document: KnowledgeDocumentDraft,
}

const CORPUS: [&str; 8] = [
    include_str!("fixtures/general-constraint.json"),
    include_str!("fixtures/runbook.json"),
    include_str!("fixtures/protocol.json"),
    include_str!("fixtures/devops.json"),
    include_str!("fixtures/operations.json"),
    include_str!("fixtures/product-research.json"),
    include_str!("fixtures/security.json"),
    include_str!("fixtures/planning-abstraction.json"),
];

fn source_ref() -> KnowledgeSourceRef {
    KnowledgeSourceRef::Snapshot {
        snapshot: KnowledgeSavedSourceSnapshot {
            title: "source".into(),
            uri: "urn:source:original".into(),
            text: "exact".into(),
            observed_at: Some("2026-09-14T00:00:00Z".into()),
            evidence_kind: KnowledgeEvidenceKind::RuntimeVerification,
        },
    }
}

fn document() -> KnowledgeDocumentDraft {
    KnowledgeDocumentDraft {
        title: "All profiles".into(),
        canonical_text: "line \"quoted\"\nsecond".into(),
        knowledge_kind: KnowledgeKind::Security,
        epistemic_state: KnowledgeEpistemicState::Observed,
        target_iris: vec!["urn:target:one".into()],
        conditions: vec![],
        exceptions: vec![],
        sources: vec![source_ref()],
        bindings: vec![KnowledgeDocumentBinding {
            target: KnowledgeBindingTarget::Workspace,
            purpose: KnowledgeBindingPurpose::Required,
            version_resolution: KnowledgeBindingVersion::CurrentAccepted,
        }],
        profiles: vec![
            KnowledgeProfileId::General,
            KnowledgeProfileId::Runbook,
            KnowledgeProfileId::Protocol,
            KnowledgeProfileId::Devops,
            KnowledgeProfileId::Operations,
            KnowledgeProfileId::ProductResearch,
            KnowledgeProfileId::Security,
        ],
        access_scope: KnowledgeAccessScope::OwnersOnly,
        owner_ref: "owner".into(),
        authority_basis: "authority".into(),
        planning_briefs: vec![],
        valid_from: None,
        valid_until: None,
        review_due_at: None,
        sections: KnowledgeProfileSections {
            constraint: None,
            general: Some(KnowledgeGeneralSection {
                statement: "statement".into(),
                assumptions: vec![],
                evidence_scope: "workspace".into(),
                rationale: "rationale".into(),
                alternatives: vec![],
                negative_limits: vec![],
                unknown_limits: vec![],
            }),
            runbook: Some(KnowledgeRunbookSection {
                purpose_and_fit: "purpose".into(),
                target_environment_iris: vec!["urn:env:test".into()],
                parameters: vec![KnowledgeParameterDefinition {
                    name: "p".into(),
                    description: "parameter".into(),
                    required: true,
                }],
                prerequisites: vec![],
                required_authority: "owner".into(),
                steps: vec![KnowledgeRunbookStep {
                    ordinal: 1,
                    action: "run".into(),
                    expected_result: "ok".into(),
                    verification: "check".into(),
                }],
                failure_and_recovery: "stop".into(),
                proof_status: KnowledgeProofStatus::RuntimeVerified,
                proof_evidence_refs: vec![0],
                dependency_iris: vec!["urn:dependency:one".into()],
            }),
            protocol: Some(KnowledgeProtocolSection {
                specification_uri: "urn:spec:one".into(),
                specification_version: "1".into(),
                provider_scope: vec!["provider".into()],
                network_scope: vec!["network".into()],
                assertions: vec![KnowledgeProtocolAssertion {
                    statement: "works".into(),
                    observed: true,
                    evidence_refs: vec![0],
                }],
                capabilities: vec![],
                compatibility_constraints: vec![],
                negative_states_and_quirks: vec![],
                observation_bounds: "bounded".into(),
            }),
            devops: Some(KnowledgeDevopsSection {
                asset_iris: vec!["urn:asset:one".into()],
                environment_iris: vec!["urn:env:test".into()],
                topology_links: vec![],
                ownership: vec!["owner".into()],
                configuration_refs: vec![],
                observations: vec![KnowledgeDatedObservation {
                    observed_at: "2026-09-14T00:00:00Z".into(),
                    status: "ready".into(),
                    limits: vec![],
                    evidence_refs: vec![0],
                }],
                deployment_surfaces: vec!["surface".into()],
                configuration_custody: "owner".into(),
            }),
            operations: Some(KnowledgeOperationsSection {
                operating_purpose: "operate".into(),
                roles: vec![KnowledgeRoleResponsibility {
                    role: "owner".into(),
                    responsibilities: vec!["operate".into()],
                }],
                cadence: "daily".into(),
                handoffs: vec![],
                escalation: vec![],
                status_semantics: vec![],
                metrics: vec![KnowledgeMetricDefinition {
                    name: "m".into(),
                    meaning: "meaning".into(),
                    objective: "objective".into(),
                    signal_source: "source".into(),
                }],
                signal_sources: vec![],
                exceptions: vec![],
                ownership_gaps: vec![],
            }),
            product_research: Some(KnowledgeProductResearchSection {
                question: "question".into(),
                evidence_map: vec![KnowledgeResearchEvidence {
                    claim: "claim".into(),
                    source_refs: vec![0],
                    synthetic: false,
                }],
                assumptions: vec![],
                segments: vec![],
                alternatives: vec![],
                conclusions_and_decisions: vec!["decision".into()],
                observation_limits: vec![],
                negative_evidence: vec![],
            }),
            security: Some(KnowledgeSecuritySection {
                asset_iris: vec!["urn:asset:one".into()],
                trust_boundaries: vec!["boundary".into()],
                threats: vec!["threat".into()],
                controls: vec!["control".into()],
                evidence_refs: vec![0],
                verification_status: "verified".into(),
                sensitivity: KnowledgeSensitivity::Restricted,
                applicable_authority: "owner".into(),
                exceptions: vec![],
                finding_state: "closed".into(),
                remediation_proof_refs: vec![],
            }),
        },
    }
}

fn input() -> RdfPublicationInput {
    let tenant = Uuid::from_u128(1);
    let workspace = Uuid::from_u128(2);
    RdfPublicationInput {
        tenant,
        workspace,
        change_id: Uuid::from_u128(3),
        event_id: Uuid::from_u128(4),
        content_revision: 1,
        planned: KnowledgePlannedOperation {
            operation_id: Uuid::from_u128(5),
            unit_id: Uuid::from_u128(6),
            client_label: "create".into(),
            operation: KnowledgeLifecycleOperation::Create,
            expected_revision: None,
            expected_lifecycle: None,
            document: Some(document()),
            revalidation: None,
            successor: None,
            replacement_bindings: vec![],
            reason: "reason".into(),
            authority_basis: "authority".into(),
            dependency_operation_ids: vec![],
            binding_pins: vec![],
        },
        principal_id: Uuid::from_u128(7),
        session_id: Uuid::from_u128(8),
        resolved_sources: vec![ResolvedSourcePayload {
            pin: KnowledgeResolvedSourcePin {
                source_index: 0,
                digest: "digest".into(),
                evidence_kind: KnowledgeEvidenceKind::RuntimeVerification,
                observed_at: Some("2026-09-14T00:00:00Z".into()),
                evidence_scope: "workspace".into(),
                source_iri: "urn:source:original".into(),
            },
            title: "resolved".into(),
            uri: "urn:source:resolved".into(),
            text: "exact bytes".into(),
        }],
        successor_unit: None,
    }
}

fn term(value: &TypedTerm) -> serde_json::Value {
    serde_json::json!({"type":value.kind,"value":value.value,"datatype":value.datatype,"language":value.language})
}

fn rows(document: &RdfDocument) -> Vec<serde_json::Value> {
    document.triples.iter().map(|triple: &TypedTriple| serde_json::json!({
        "subject":term(&triple.subject),"predicate":term(&triple.predicate),"object":term(&triple.object)
    })).collect()
}

fn corpus_input(fixture: &CorpusFixture, ordinal: u128) -> RdfPublicationInput {
    let snapshot = match &fixture.document.sources[0] {
        KnowledgeSourceRef::Snapshot { snapshot } => snapshot,
        KnowledgeSourceRef::PipelineOutput { .. } => panic!("corpus source must be a snapshot"),
    };
    RdfPublicationInput {
        tenant: Uuid::from_u128(101),
        workspace: Uuid::from_u128(102),
        change_id: Uuid::from_u128(200 + ordinal),
        event_id: Uuid::from_u128(300 + ordinal),
        content_revision: 1,
        planned: KnowledgePlannedOperation {
            operation_id: Uuid::from_u128(400 + ordinal),
            unit_id: Uuid::from_u128(500 + ordinal),
            client_label: fixture.id.clone(),
            operation: KnowledgeLifecycleOperation::Create,
            expected_revision: None,
            expected_lifecycle: None,
            document: Some(fixture.document.clone()),
            revalidation: None,
            successor: None,
            replacement_bindings: vec![],
            reason: "owner-authorized corpus acceptance".into(),
            authority_basis: fixture.document.authority_basis.clone(),
            dependency_operation_ids: vec![],
            binding_pins: vec![],
        },
        principal_id: Uuid::from_u128(103),
        session_id: Uuid::from_u128(104),
        resolved_sources: vec![ResolvedSourcePayload {
            pin: KnowledgeResolvedSourcePin {
                source_index: 0,
                digest: fixture.source_sha256.clone(),
                evidence_kind: snapshot.evidence_kind,
                observed_at: snapshot.observed_at.clone(),
                evidence_scope: fixture.document.conditions[0].clone(),
                source_iri: snapshot.uri.clone(),
            },
            title: snapshot.title.clone(),
            uri: snapshot.uri.clone(),
            text: snapshot.text.clone(),
        }],
        successor_unit: None,
    }
}

fn required_section(fixture: &CorpusFixture) -> &'static str {
    match fixture.document.knowledge_kind {
        KnowledgeKind::Constraint => "constraint",
        KnowledgeKind::Procedure => "runbook",
        KnowledgeKind::Protocol => "protocol",
        KnowledgeKind::Infrastructure => "devops",
        KnowledgeKind::OperatingModel => "operations",
        KnowledgeKind::ProductResearch => "product_research",
        KnowledgeKind::Security => "security",
        _ => panic!("unexpected corpus kind"),
    }
}

#[test]
fn source_corpus_documents_roundtrip_as_exact_typed_sets() {
    for (index, raw) in CORPUS.iter().enumerate() {
        let fixture: CorpusFixture = serde_json::from_str(raw).unwrap();
        assert!(!fixture.source_path.is_empty());
        assert!(!fixture.source_revision.is_empty());
        let snapshot = match &fixture.document.sources[0] {
            KnowledgeSourceRef::Snapshot { snapshot } => snapshot,
            _ => unreachable!(),
        };
        assert_eq!(
            format!("{:x}", Sha256::digest(snapshot.text.as_bytes())),
            fixture.source_sha256
        );
        let document = build(&corpus_input(&fixture, index as u128)).unwrap();
        assert_eq!(validate_rows(&rows(&document), &document), Ok(()));
        assert!(!document.payload.contains(&format!("<{V2}generalSection>")));
        let section = required_section(&fixture);
        let section_iri = format!("{}:section:{section}", document.refs.revision);
        assert!(document.payload.contains(&format!("<{section_iri}>")));
        let without_section = rows(&document)
            .into_iter()
            .filter(|row| row["subject"]["value"] != section_iri)
            .collect::<Vec<_>>();
        assert_eq!(
            validate_rows(&without_section, &document),
            Err(Error::InternalInvariant),
            "{} whole required section must be detected",
            fixture.id
        );
    }
}

#[test]
fn revalidation_is_event_only_and_exactly_typed() {
    let fixture: CorpusFixture = serde_json::from_str(CORPUS[3]).unwrap();
    let mut value = corpus_input(&fixture, 8);
    let source = fixture.document.sources[0].clone();
    value.planned.operation = KnowledgeLifecycleOperation::Revalidate;
    value.planned.expected_revision = Some(1);
    value.planned.expected_lifecycle = Some(KnowledgeLifecycleState::Active);
    value.planned.document = None;
    value.planned.revalidation = Some(KnowledgeRevalidationDraft {
        sources: vec![source],
        evidence_basis: "fresh isolated exact publication/read proof".into(),
        valid_until: None,
        review_due_at: None,
    });
    let document = build(&value).unwrap();
    assert!(
        document
            .payload
            .contains(&format!("<{V2}PublicationEvent>"))
    );
    assert!(
        !document
            .payload
            .contains(&format!("<{V2}KnowledgeRevision>"))
    );
    assert!(!document.payload.contains("<urn:tect:dk:hasRevision>"));
    assert_eq!(validate_rows(&rows(&document), &document), Ok(()));
}

#[test]
fn slice_phase_binding_requires_and_encodes_the_server_pin() {
    let mut value = input();
    let scope = Uuid::from_u128(900);
    let slice = Uuid::from_u128(901);
    value.planned.document.as_mut().unwrap().bindings = vec![KnowledgeDocumentBinding {
        target: KnowledgeBindingTarget::SlicePhase {
            scope_id: scope,
            slice_id: slice,
            phase_id: "verify".into(),
        },
        purpose: KnowledgeBindingPurpose::ProofBasis,
        version_resolution: KnowledgeBindingVersion::CurrentAccepted,
    }];
    assert!(matches!(build(&value), Err(Error::InvalidArguments)));
    value.planned.binding_pins = vec![KnowledgeResolvedBindingPin {
        binding_index: 0,
        definition_kind: PipelineKind::PromoteToDurableKnowledge,
        definition_version: "dk2".into(),
        definition_digest: "digest".into(),
        phase_id: "verify".into(),
    }];
    let document = build(&value).unwrap();
    assert!(document.payload.contains(&format!(
        "<{V2}definitionKind> <{V2}pipeline-kind:slice.promote-to-durable-knowledge>"
    )));
    assert!(
        document
            .payload
            .contains(&format!("<{V2}definitionVersion> \"dk2\""))
    );
    assert!(
        document
            .payload
            .contains(&format!("<{V2}definitionDigest> \"digest\""))
    );
}

#[test]
fn all_profiles_are_deterministic_ordered_typed_and_escaped() {
    let value = input();
    let first = build(&value).unwrap();
    let second = build(&value).unwrap();
    assert_eq!(first.payload, second.payload);
    assert!(first.payload.contains("line \\\"quoted\\\"\\nsecond"));
    for class in [
        "GeneralSection",
        "RunbookSection",
        "ProtocolSection",
        "DevopsSection",
        "OperationsSection",
        "ProductResearchSection",
        "SecuritySection",
    ] {
        assert!(first.payload.contains(&format!("<{V2}{class}>")));
    }
    assert!(first.payload.contains(&format!(
        "<{V2}ordinal> \"1\"^^<http://www.w3.org/2001/XMLSchema#integer>"
    )));
    assert!(!first.payload.contains("canonicalPayload"));
    assert_eq!(validate_rows(&rows(&first), &first), Ok(()));
}

#[test]
fn exact_typed_set_rejects_missing_extra_and_wrong_term_type() {
    let document = build(&input()).unwrap();
    let original = rows(&document);
    let mut missing = original.clone();
    missing.pop();
    assert_eq!(
        validate_rows(&missing, &document),
        Err(Error::InternalInvariant)
    );
    let mut extra = original.clone();
    extra.push(original[0].clone());
    extra.last_mut().unwrap()["predicate"]["value"] = serde_json::json!(format!("{V2}unknown"));
    assert_eq!(
        validate_rows(&extra, &document),
        Err(Error::InternalInvariant)
    );
    let mut duplicate = rows(&document);
    duplicate.push(duplicate[0].clone());
    assert_eq!(
        validate_rows(&duplicate, &document),
        Err(Error::InternalInvariant)
    );
    let mut wrong_type = original;
    let row = wrong_type
        .iter_mut()
        .find(|row| row["predicate"]["value"] == format!("{V2}target"))
        .unwrap();
    row["object"]["type"] = serde_json::json!("literal");
    row["object"]["datatype"] = serde_json::json!("http://www.w3.org/2001/XMLSchema#string");
    assert_eq!(
        validate_rows(&wrong_type, &document),
        Err(Error::InternalInvariant)
    );
}

#[test]
fn revise_keeps_legacy_unit_scaffold_without_reinserting_legacy_type() {
    let mut value = input();
    value.content_revision = 2;
    value.planned.operation = KnowledgeLifecycleOperation::Revise;
    value.planned.expected_revision = Some(1);
    value.planned.expected_lifecycle = Some(KnowledgeLifecycleState::Active);
    let document = build(&value).unwrap();
    let legacy_type = format!(
        "<{}> <{}> <urn:tect:dk:KnowledgeUnit>",
        document.refs.unit,
        super::model::RDF_TYPE
    );
    assert!(document.payload.contains(&legacy_type));
    assert!(!document.stable_payload.contains(&legacy_type));
    assert!(
        document
            .stable_payload
            .contains("<urn:tect:dk:hasRevision>")
    );
    assert!(!document.payload.contains("urn:tect:dk:v2:KnowledgeUnit"));
}
