use super::corpus::{SearchEdge, SearchResource};
use super::*;

pub(super) fn legacy(
    value: KnowledgeUnitRevision,
    access_scope: KnowledgeAccessScope,
) -> Result<SearchResource> {
    let resource_iri = value.unit_iri.clone();
    let revision_iri = value.revision_iri.clone();
    let target = value.constraint.target_iri.clone();
    let source_iri = value.source_iri.clone();
    let verified_payload_bytes = serde_json::to_vec(&value.constraint)
        .map_err(storage_error)?
        .len();
    Ok(SearchResource {
        unit_id: value.unit_id,
        resource_iri: resource_iri.clone(),
        revision: value.revision,
        revision_iri,
        title: value.constraint.title,
        canonical_text: value.constraint.statement,
        kind: KnowledgeKind::Constraint,
        lifecycle: KnowledgeLifecycleState::Active,
        access_scope,
        contract_version: "dk-1".into(),
        verified_payload_bytes,
        source_digests: vec![value.source_sha256],
        freshness_warnings: Vec::new(),
        edges: vec![
            edge(
                &resource_iri,
                &target,
                KnowledgeSearchRelation::Targets,
                vec!["urn:tect:dk:target"],
            ),
            edge(
                &resource_iri,
                &source_iri,
                KnowledgeSearchRelation::DerivedFrom,
                vec!["urn:tect:dk:source"],
            ),
        ],
        visible_internal_endpoints: vec![resource_iri, source_iri],
    })
}

fn edge(from: &str, to: &str, relation: KnowledgeSearchRelation, path: Vec<&str>) -> SearchEdge {
    SearchEdge {
        from: from.into(),
        to: to.into(),
        relation,
        predicate_path: path.into_iter().map(String::from).collect(),
        binding: None,
    }
}

pub(super) fn dk2(
    tenant: Uuid,
    workspace: Uuid,
    value: KnowledgeDocumentRevision,
    sources: Vec<crate::knowledge_lifecycle::rdf::ResolvedSourcePayload>,
    freshness_warnings: Vec<String>,
) -> Result<SearchResource> {
    let resource = value.unit_iri.clone();
    let revision = value.revision_iri.clone();
    let document = value.document;
    let mut edges = graph_edges(tenant, workspace, &resource, &document, &sources)?;
    edges.sort_by(|a, b| {
        (a.from.as_str(), a.to.as_str(), a.relation).cmp(&(
            b.from.as_str(),
            b.to.as_str(),
            b.relation,
        ))
    });
    let mut visible_internal_endpoints = vec![resource.clone()];
    visible_internal_endpoints.extend(sources.iter().map(|value| value.pin.source_iri.clone()));
    let verified_payload_bytes = serde_json::to_vec(&document).map_err(storage_error)?.len();
    Ok(SearchResource {
        unit_id: value.unit_id,
        resource_iri: resource,
        revision: value.revision,
        revision_iri: revision,
        title: document.title,
        canonical_text: document.canonical_text,
        kind: document.knowledge_kind,
        lifecycle: value.lifecycle,
        access_scope: document.access_scope,
        contract_version: "dk-2".into(),
        verified_payload_bytes,
        source_digests: value.source_digests,
        freshness_warnings,
        edges,
        visible_internal_endpoints,
    })
}

fn graph_edges(
    tenant: Uuid,
    workspace: Uuid,
    resource: &str,
    doc: &KnowledgeDocumentDraft,
    sources: &[crate::knowledge_lifecycle::rdf::ResolvedSourcePayload],
) -> Result<Vec<SearchEdge>> {
    let v2 = "urn:tect:dk:v2:";
    let mut values = Vec::new();
    for iri in &doc.target_iris {
        values.push(edge(
            resource,
            iri,
            KnowledgeSearchRelation::Targets,
            vec![
                &format!("{v2}targets"),
                &format!("{v2}entry"),
                &format!("{v2}iriValue"),
            ],
        ));
    }
    if let Some(runbook) = &doc.sections.runbook {
        for iri in &runbook.dependency_iris {
            values.push(edge(
                resource,
                iri,
                KnowledgeSearchRelation::DependsOn,
                vec![
                    &format!("{v2}runbookSection"),
                    &format!("{v2}dependencies"),
                    &format!("{v2}entry"),
                    &format!("{v2}iriValue"),
                ],
            ));
        }
        for iri in &runbook.target_environment_iris {
            values.push(edge(
                resource,
                iri,
                KnowledgeSearchRelation::InEnvironment,
                vec![
                    &format!("{v2}runbookSection"),
                    &format!("{v2}targetEnvironments"),
                    &format!("{v2}entry"),
                    &format!("{v2}iriValue"),
                ],
            ));
        }
    }
    if let Some(devops) = &doc.sections.devops {
        for iri in &devops.asset_iris {
            values.push(edge(
                resource,
                iri,
                KnowledgeSearchRelation::UsesAsset,
                vec![
                    &format!("{v2}devopsSection"),
                    &format!("{v2}assets"),
                    &format!("{v2}entry"),
                    &format!("{v2}iriValue"),
                ],
            ));
        }
        for iri in &devops.environment_iris {
            values.push(edge(
                resource,
                iri,
                KnowledgeSearchRelation::InEnvironment,
                vec![
                    &format!("{v2}devopsSection"),
                    &format!("{v2}environments"),
                    &format!("{v2}entry"),
                    &format!("{v2}iriValue"),
                ],
            ));
        }
    }
    if let Some(security) = &doc.sections.security {
        for iri in &security.asset_iris {
            values.push(edge(
                resource,
                iri,
                KnowledgeSearchRelation::UsesAsset,
                vec![
                    &format!("{v2}securitySection"),
                    &format!("{v2}assets"),
                    &format!("{v2}entry"),
                    &format!("{v2}iriValue"),
                ],
            ));
        }
    }
    for source in sources {
        values.push(edge(
            resource,
            &source.pin.source_iri,
            KnowledgeSearchRelation::DerivedFrom,
            vec![
                &format!("{v2}sources"),
                &format!("{v2}entry"),
                &format!("{v2}originalSource"),
            ],
        ));
        if source.uri != source.pin.source_iri {
            values.push(edge(
                resource,
                &source.uri,
                KnowledgeSearchRelation::DerivedFrom,
                vec![
                    &format!("{v2}sources"),
                    &format!("{v2}entry"),
                    &format!("{v2}uri"),
                ],
            ));
        }
    }
    for binding in &doc.bindings {
        let (target, qualifier) = binding_edge(tenant, workspace, binding)?;
        let mut value = edge(
            resource,
            &target,
            KnowledgeSearchRelation::BoundTo,
            vec![
                &format!("{v2}bindings"),
                &format!("{v2}entry"),
                &format!("{v2}target"),
            ],
        );
        value.binding = Some(qualifier);
        values.push(value);
    }
    Ok(values)
}

fn binding_edge(
    tenant: Uuid,
    workspace: Uuid,
    value: &KnowledgeDocumentBinding,
) -> Result<(String, KnowledgeGraphBindingQualifier)> {
    let root = format!("urn:tect:workspace:{tenant}:{workspace}");
    let target = match &value.target {
        KnowledgeBindingTarget::Workspace => root,
        KnowledgeBindingTarget::Program { program_id } => format!("{root}:program:{program_id}"),
        KnowledgeBindingTarget::Scope { scope_id } => format!("{root}:scope:{scope_id}"),
        KnowledgeBindingTarget::Slice { scope_id, slice_id } => {
            format!("{root}:scope:{scope_id}:slice:{slice_id}")
        }
        KnowledgeBindingTarget::SlicePhase {
            scope_id, slice_id, ..
        } => format!("{root}:scope:{scope_id}:slice:{slice_id}:phase"),
    };
    Ok((
        target,
        KnowledgeGraphBindingQualifier {
            purpose: value.purpose,
            version_resolution: value.version_resolution.clone(),
            phase_id: match &value.target {
                KnowledgeBindingTarget::SlicePhase { phase_id, .. } => Some(phase_id.clone()),
                _ => None,
            },
        },
    ))
}
