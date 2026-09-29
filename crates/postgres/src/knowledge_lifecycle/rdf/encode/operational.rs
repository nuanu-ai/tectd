use super::super::legacy_operational::*;
use super::super::model::{Builder, RDF_TYPE};
use serde::Serialize;
use tect_domain::{Error, Result};

const OPS: &str = "urn:tectd:vocab:ops:v1:";

fn term<T: Serialize>(category: &str, value: T) -> Result<String> {
    let value = serde_json::to_value(value).map_err(|_| Error::InternalInvariant)?;
    let value = value.as_str().ok_or(Error::InternalInvariant)?;
    Ok(format!("{OPS}{category}:{value}"))
}

fn property(name: &str) -> String {
    format!("{OPS}{name}")
}

fn optional_date(builder: &mut Builder, node: &str, name: &str, date: Option<&str>) -> Result<()> {
    if let Some(date) = date {
        builder.datetime(node, &property(name), date)?;
    }
    Ok(())
}

fn source_refs(builder: &mut Builder, parent: &str, refs: &[OperationalSourceRef]) -> Result<()> {
    for (index, reference) in refs.iter().enumerate() {
        let node = format!("{parent}:source:{}", index + 1);
        builder.iri(parent, &property("sourceRef"), &node)?;
        builder.iri(&node, RDF_TYPE, &property("SourceFragmentReference"))?;
        builder.integer(
            &node,
            &property("sourceIndex"),
            reference.source_index as i64,
        )?;
        builder.iri(&node, &property("fragmentIri"), &reference.fragment_iri)?;
        builder.text(&node, &property("sha256"), &reference.sha256)?;
    }
    Ok(())
}

pub(super) fn encode(
    builder: &mut Builder,
    revision: &str,
    refs: &OperationalReferencesDraft,
) -> Result<()> {
    builder.integer(
        revision,
        &property("schemaVersion"),
        OPERATIONAL_SCHEMA_VERSION as i64,
    )?;
    if let Some(entity) = &refs.entity {
        let node = format!("{revision}:operational:entity");
        builder.iri(revision, &property("operationalEntity"), &node)?;
        builder.iri(&node, RDF_TYPE, &property("OperationalEntityRevision"))?;
        builder.iri(&node, RDF_TYPE, &property(class_name(entity.kind())))?;
        builder.iri(&node, &property("stableIri"), entity.iri())?;
        builder.iri(&node, &property("owningRevision"), revision)?;
        source_refs(builder, &node, &refs.entity_source_refs)?;
        match entity {
            OperationalEntityDraft::KnowledgeResource { .. } => {}
            OperationalEntityDraft::TaxonomyConcept {
                label,
                locale,
                aliases,
                preferred_labels,
                localized_aliases,
                ..
            } => {
                builder.text(&node, &property("label"), label)?;
                builder.text(&node, &property("locale"), locale)?;
                for alias in aliases {
                    builder.text(&node, &property("alias"), alias)?;
                }
                for (index, localized) in preferred_labels.iter().enumerate() {
                    let term = format!("{node}:preferred-label:{}", index + 1);
                    builder.iri(&node, &property("preferredLabel"), &term)?;
                    builder.text(&term, &property("locale"), &localized.locale)?;
                    builder.text(&term, &property("label"), &localized.label)?;
                }
                for (index, localized) in localized_aliases.iter().enumerate() {
                    let term = format!("{node}:localized-alias:{}", index + 1);
                    builder.iri(&node, &property("localizedAlias"), &term)?;
                    builder.text(&term, &property("locale"), &localized.locale)?;
                    builder.text(&term, &property("label"), &localized.label)?;
                }
            }
            OperationalEntityDraft::ProjectContext {
                label,
                custodian_ref,
                ..
            } => {
                builder.text(&node, &property("label"), label)?;
                builder.text(&node, &property("custodianRef"), custodian_ref)?;
            }
            OperationalEntityDraft::Environment {
                label,
                environment_kind,
                project_context_iri,
                ..
            } => {
                builder.text(&node, &property("label"), label)?;
                builder.iri(
                    &node,
                    &property("environmentKind"),
                    &term("environment-kind", environment_kind)?,
                )?;
                builder.iri(&node, &property("projectContextIri"), project_context_iri)?;
            }
            OperationalEntityDraft::DeploymentSurface {
                label,
                environment_iri,
                surface_kind,
                declared_state,
                current_state,
                ..
            } => {
                builder.text(&node, &property("label"), label)?;
                builder.iri(&node, &property("environmentIri"), environment_iri)?;
                builder.text(&node, &property("surfaceKind"), surface_kind)?;
                builder.text(&node, &property("declaredState"), declared_state)?;
                builder.text(&node, &property("currentState"), current_state)?;
            }
            OperationalEntityDraft::Host {
                label,
                identity_evidence_refs,
                provider_instance_id,
                ..
            } => {
                builder.text(&node, &property("label"), label)?;
                for source_index in identity_evidence_refs {
                    builder.integer(
                        &node,
                        &property("identityEvidenceSourceIndex"),
                        *source_index as i64,
                    )?;
                }
                if let Some(id) = provider_instance_id {
                    builder.text(&node, &property("providerInstanceId"), id)?;
                }
            }
            OperationalEntityDraft::AccessRoute {
                purpose,
                origin_iri,
                observed_at,
                review_due_at,
                ..
            } => {
                builder.iri(&node, &property("purpose"), &term("purpose", purpose)?)?;
                builder.iri(&node, &property("originIri"), origin_iri)?;
                builder.datetime(&node, &property("observedAt"), observed_at)?;
                builder.datetime(&node, &property("reviewDueAt"), review_due_at)?;
            }
            OperationalEntityDraft::NetworkEndpoint {
                endpoint_kind,
                locator,
                observed_at,
                review_due_at,
                ..
            } => {
                builder.iri(
                    &node,
                    &property("endpointKind"),
                    &term("endpoint-kind", endpoint_kind)?,
                )?;
                builder.text(&node, &property("locator"), locator)?;
                builder.datetime(&node, &property("observedAt"), observed_at)?;
                builder.datetime(&node, &property("reviewDueAt"), review_due_at)?;
            }
            OperationalEntityDraft::CredentialLocator {
                locator_kind,
                locator,
                custodian_ref,
                observed_at,
                review_due_at,
                ..
            } => {
                builder.iri(
                    &node,
                    &property("locatorKind"),
                    &term("locator-kind", locator_kind)?,
                )?;
                builder.text(&node, &property("locator"), locator)?;
                builder.text(&node, &property("custodianRef"), custodian_ref)?;
                builder.datetime(&node, &property("observedAt"), observed_at)?;
                builder.datetime(&node, &property("reviewDueAt"), review_due_at)?;
            }
            OperationalEntityDraft::AccessProcedure {
                purpose,
                target_iri,
                target_kind,
                ..
            } => {
                builder.iri(&node, &property("purpose"), &term("purpose", purpose)?)?;
                builder.iri(&node, &property("targetIri"), target_iri)?;
                builder.iri(
                    &node,
                    &property("targetKind"),
                    &property(class_name(*target_kind)),
                )?;
            }
        }
    }
    for (index, assertion) in refs.assertions.iter().enumerate() {
        let node = format!("{revision}:operational:assertion:{}", index + 1);
        builder.iri(revision, &property("operationalAssertion"), &node)?;
        builder.iri(
            &node,
            RDF_TYPE,
            &property(
                if assertion.predicate == OperationalPredicate::ClassifiedAs {
                    "ConceptAssignment"
                } else {
                    "TopologyAssertion"
                },
            ),
        )?;
        builder.iri(&node, &property("owningRevision"), revision)?;
        builder.iri(&node, &property("subject"), &assertion.subject_iri)?;
        builder.iri(
            &node,
            &property("subjectClass"),
            &property(class_name(assertion.subject_kind)),
        )?;
        builder.iri(
            &node,
            &property("predicate"),
            &property(assertion.predicate.rdf_local_name()),
        )?;
        builder.iri(&node, &property("object"), &assertion.object_iri)?;
        builder.iri(
            &node,
            &property("objectClass"),
            &property(class_name(assertion.object_kind)),
        )?;
        builder.iri(&node, &property("state"), &term("state", assertion.state)?)?;
        builder.iri(
            &node,
            &property("accessScope"),
            &super::enum_iri("access", assertion.access_scope)?,
        )?;
        builder.text(&node, &property("reviewerRef"), &assertion.reviewer_ref)?;
        builder.text(&node, &property("reviewReceipt"), &assertion.review_receipt)?;
        builder.text(
            &node,
            &property("authorityBasis"),
            &assertion.authority_basis,
        )?;
        builder.datetime(&node, &property("assertedAt"), &assertion.asserted_at)?;
        optional_date(
            builder,
            &node,
            "observedAt",
            assertion.observed_at.as_deref(),
        )?;
        optional_date(builder, &node, "validFrom", assertion.valid_from.as_deref())?;
        optional_date(
            builder,
            &node,
            "validUntil",
            assertion.valid_until.as_deref(),
        )?;
        optional_date(
            builder,
            &node,
            "reviewDueAt",
            assertion.review_due_at.as_deref(),
        )?;
        if let Some(purpose) = assertion.route_purpose {
            builder.iri(&node, &property("routePurpose"), &term("purpose", purpose)?)?;
        }
        if let Some(purpose) = assertion.procedure_purpose {
            builder.iri(
                &node,
                &property("procedurePurpose"),
                &term("purpose", purpose)?,
            )?;
        }
        source_refs(builder, &node, &assertion.source_refs)?;
    }
    Ok(())
}

fn class_name(kind: OperationalEntityKind) -> &'static str {
    use OperationalEntityKind::*;
    match kind {
        TaxonomyConcept => "TaxonomyConcept",
        KnowledgeResource => "KnowledgeResource",
        ProjectContext => "ProjectContext",
        Environment => "Environment",
        DeploymentSurface => "DeploymentSurface",
        Host => "Host",
        AccessRoute => "AccessRoute",
        NetworkEndpoint => "NetworkEndpoint",
        CredentialLocator => "CredentialLocator",
        AccessProcedure => "AccessProcedure",
    }
}
