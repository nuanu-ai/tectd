use super::model::{Builder, DK, RDF_TYPE, RdfDocument, RdfRefs, V2};
use super::sections;
use super::{RdfPublicationInput, ResolvedSourcePayload};
use serde::Serialize;
use tect_domain::*;

mod planning;

pub(super) fn build(input: &RdfPublicationInput) -> Result<RdfDocument> {
    input.planned.validate()?;
    if input.content_revision < 1 || input.change_id.is_nil() || input.event_id.is_nil() {
        return Err(Error::InvalidArguments);
    }
    let refs = refs(input);
    if input.planned.operation == KnowledgeLifecycleOperation::Erase {
        return Builder::new(refs).finish(false);
    }
    let mut builder = Builder::new(refs);
    match input.planned.operation {
        KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise => {
            build_revision(&mut builder, input)?;
        }
        KnowledgeLifecycleOperation::Revalidate
        | KnowledgeLifecycleOperation::Supersede
        | KnowledgeLifecycleOperation::Retract => build_event(&mut builder, input)?,
        KnowledgeLifecycleOperation::Erase => unreachable!(),
    }
    let revise = input.planned.operation == KnowledgeLifecycleOperation::Revise;
    builder.finish(revise)
}

fn refs(input: &RdfPublicationInput) -> RdfRefs {
    let unit = format!(
        "urn:tect:dk:unit:{}:{}:{}",
        input.tenant, input.workspace, input.planned.unit_id
    );
    RdfRefs {
        revision: format!("{unit}:revision:{}", input.content_revision),
        event: format!(
            "urn:tect:dk:event:{}:{}:{}",
            input.tenant, input.workspace, input.event_id
        ),
        event_content: format!("{unit}:event:{}", input.event_id),
        unit,
    }
}

fn build_revision(builder: &mut Builder, input: &RdfPublicationInput) -> Result<()> {
    let document = input
        .planned
        .document
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    if input.resolved_sources.len() != document.sources.len()
        || input
            .resolved_sources
            .iter()
            .enumerate()
            .any(|(index, source)| {
                source.pin.source_index as usize != index || source.pin.source_iri.trim().is_empty()
            })
    {
        return Err(Error::InvalidArguments);
    }
    let unit = builder.refs.unit.clone();
    let revision = builder.refs.revision.clone();
    builder.iri(&unit, RDF_TYPE, &format!("{DK}KnowledgeUnit"))?;
    builder.iri(&unit, &format!("{DK}hasRevision"), &revision)?;
    builder.iri(&revision, RDF_TYPE, &format!("{V2}KnowledgeRevision"))?;
    builder.iri(&revision, &field("unit"), &unit)?;
    builder.integer(&revision, &field("contentRevision"), input.content_revision)?;
    builder.text(&revision, &field("title"), &document.title)?;
    builder.text(&revision, &field("canonicalText"), &document.canonical_text)?;
    builder.iri(
        &revision,
        &field("kind"),
        &enum_iri("kind", document.knowledge_kind)?,
    )?;
    builder.iri(
        &revision,
        &field("epistemicState"),
        &enum_iri("epistemic", document.epistemic_state)?,
    )?;
    builder.iri(
        &revision,
        &field("accessScope"),
        &enum_iri("access", document.access_scope)?,
    )?;
    builder.text(&revision, &field("ownerRef"), &document.owner_ref)?;
    builder.text(
        &revision,
        &field("authorityBasis"),
        &document.authority_basis,
    )?;
    optional_datetime(
        builder,
        &revision,
        "validFrom",
        document.valid_from.as_deref(),
    )?;
    optional_datetime(
        builder,
        &revision,
        "validUntil",
        document.valid_until.as_deref(),
    )?;
    optional_datetime(
        builder,
        &revision,
        "reviewDueAt",
        document.review_due_at.as_deref(),
    )?;
    list_iris(
        builder,
        &revision,
        "targets",
        &revision,
        &document.target_iris,
    )?;
    list_text(
        builder,
        &revision,
        "conditions",
        &revision,
        &document.conditions,
    )?;
    list_text(
        builder,
        &revision,
        "exceptions",
        &revision,
        &document.exceptions,
    )?;
    let profile_iris = document
        .profiles
        .iter()
        .map(|profile| enum_iri("profile", *profile))
        .collect::<Result<Vec<_>>>()?;
    list_iris(builder, &revision, "profiles", &revision, &profile_iris)?;
    for profile in &profile_iris {
        builder.iri(&revision, &field("profile"), profile)?;
    }
    encode_sources(builder, &revision, &revision, &input.resolved_sources)?;
    encode_bindings(builder, &revision, &revision, &document.bindings, input)?;
    planning::encode_planning_briefs(builder, &revision, document)?;
    sections::encode(builder, &revision, document)?;
    build_event(builder, input)?;
    builder.iri(
        &builder.refs.event.clone(),
        &field("revisionRef"),
        &revision,
    )
}

fn build_event(builder: &mut Builder, input: &RdfPublicationInput) -> Result<()> {
    let event = builder.refs.event.clone();
    let event_content = builder.refs.event_content.clone();
    let unit = builder.refs.unit.clone();
    builder.iri(&event, RDF_TYPE, &format!("{V2}PublicationEvent"))?;
    builder.iri(&event, &field("unit"), &unit)?;
    builder.iri(
        &event,
        &field("change"),
        &format!(
            "urn:tect:dk:change:{}:{}:{}",
            input.tenant, input.workspace, input.change_id
        ),
    )?;
    builder.iri(
        &event,
        &field("operation"),
        &enum_iri("operation", input.planned.operation)?,
    )?;
    builder.integer(&event, &field("contentRevision"), input.content_revision)?;
    builder.text(&event, &field("reason"), &input.planned.reason)?;
    builder.text(
        &event,
        &field("authorityBasis"),
        &input.planned.authority_basis,
    )?;
    builder.iri(
        &event,
        &field("actorPrincipal"),
        &format!("urn:tect:principal:{}", input.principal_id),
    )?;
    builder.iri(
        &event,
        &field("actorSession"),
        &format!("urn:tect:session:{}", input.session_id),
    )?;
    let dependencies = input
        .planned
        .dependency_operation_ids
        .iter()
        .map(|id| format!("urn:tect:dk:change-operation:{}:{id}", input.change_id))
        .collect::<Vec<_>>();
    list_iris(
        builder,
        &event,
        "dependencies",
        &event_content,
        &dependencies,
    )?;
    match input.planned.operation {
        KnowledgeLifecycleOperation::Revalidate => {
            let draft = input
                .planned
                .revalidation
                .as_ref()
                .ok_or(Error::InvalidArguments)?;
            if draft.sources.len() != input.resolved_sources.len() {
                return Err(Error::InvalidArguments);
            }
            builder.text(&event, &field("evidenceBasis"), &draft.evidence_basis)?;
            optional_datetime(builder, &event, "validUntil", draft.valid_until.as_deref())?;
            optional_datetime(
                builder,
                &event,
                "reviewDueAt",
                draft.review_due_at.as_deref(),
            )?;
            encode_sources(builder, &event, &event_content, &input.resolved_sources)?;
        }
        KnowledgeLifecycleOperation::Supersede => {
            let successor = input.successor_unit.ok_or(Error::InvalidArguments)?;
            builder.iri(
                &event,
                &field("successorUnit"),
                &format!(
                    "urn:tect:dk:unit:{}:{}:{successor}",
                    input.tenant, input.workspace
                ),
            )?;
            encode_bindings(
                builder,
                &event,
                &event_content,
                &input.planned.replacement_bindings,
                input,
            )?;
        }
        KnowledgeLifecycleOperation::Create
        | KnowledgeLifecycleOperation::Revise
        | KnowledgeLifecycleOperation::Retract => {}
        KnowledgeLifecycleOperation::Erase => return Err(Error::InvalidArguments),
    }
    Ok(())
}

fn encode_sources(
    builder: &mut Builder,
    parent: &str,
    base: &str,
    sources: &[ResolvedSourcePayload],
) -> Result<()> {
    let nodes = structured_list(builder, parent, "sources", base, sources.len(), "Source")?;
    for (node, source) in nodes.iter().zip(sources) {
        builder.integer(node, &field("sourceIndex"), source.pin.source_index as i64)?;
        builder.text(node, &field("digest"), &source.pin.digest)?;
        builder.iri(
            node,
            &field("evidenceKind"),
            &enum_iri("evidence", source.pin.evidence_kind)?,
        )?;
        optional_datetime(
            builder,
            node,
            "observedAt",
            source.pin.observed_at.as_deref(),
        )?;
        builder.text(node, &field("evidenceScope"), &source.pin.evidence_scope)?;
        builder.iri(node, &field("originalSource"), &source.pin.source_iri)?;
        builder.text(node, &field("title"), &source.title)?;
        builder.iri(node, &field("uri"), &source.uri)?;
        builder.text(node, &field("text"), &source.text)?;
    }
    Ok(())
}

fn encode_bindings(
    builder: &mut Builder,
    parent: &str,
    base: &str,
    bindings: &[KnowledgeDocumentBinding],
    input: &RdfPublicationInput,
) -> Result<()> {
    let nodes = structured_list(builder, parent, "bindings", base, bindings.len(), "Binding")?;
    for (index, (node, binding)) in nodes.iter().zip(bindings).enumerate() {
        builder.iri(
            node,
            &field("purpose"),
            &enum_iri("binding-purpose", binding.purpose)?,
        )?;
        match binding.version_resolution {
            KnowledgeBindingVersion::CurrentAccepted => builder.iri(
                node,
                &field("versionResolution"),
                &format!("{V2}binding-version:current_accepted"),
            )?,
            KnowledgeBindingVersion::PinnedRevision { revision } => {
                builder.iri(
                    node,
                    &field("versionResolution"),
                    &format!("{V2}binding-version:pinned_revision"),
                )?;
                builder.integer(node, &field("pinnedRevision"), revision)?;
            }
        }
        let (target, phase) = binding_target(binding, input);
        builder.iri(node, &field("target"), &target)?;
        if let Some(phase) = phase {
            builder.text(node, &field("phaseId"), phase)?;
            let pin = input
                .planned
                .binding_pins
                .iter()
                .find(|pin| pin.binding_index as usize == index)
                .ok_or(Error::InvalidArguments)?;
            builder.iri(
                node,
                &field("definitionKind"),
                &format!("{V2}pipeline-kind:{}", pin.definition_kind.as_str()),
            )?;
            builder.text(node, &field("definitionVersion"), &pin.definition_version)?;
            builder.text(node, &field("definitionDigest"), &pin.definition_digest)?;
        }
    }
    Ok(())
}

fn binding_target<'a>(
    binding: &'a KnowledgeDocumentBinding,
    input: &RdfPublicationInput,
) -> (String, Option<&'a str>) {
    let root = format!("urn:tect:workspace:{}:{}", input.tenant, input.workspace);
    match &binding.target {
        KnowledgeBindingTarget::Workspace => (root, None),
        KnowledgeBindingTarget::Program { program_id } => {
            (format!("{root}:program:{program_id}"), None)
        }
        KnowledgeBindingTarget::Scope { scope_id } => (format!("{root}:scope:{scope_id}"), None),
        KnowledgeBindingTarget::Slice { scope_id, slice_id } => {
            (format!("{root}:scope:{scope_id}:slice:{slice_id}"), None)
        }
        KnowledgeBindingTarget::SlicePhase {
            scope_id,
            slice_id,
            phase_id,
        } => (
            format!("{root}:scope:{scope_id}:slice:{slice_id}:phase"),
            Some(phase_id),
        ),
    }
}

pub(super) fn field(name: &str) -> String {
    format!("{V2}{name}")
}

pub(super) fn enum_iri<T: Serialize>(category: &str, value: T) -> Result<String> {
    let value = serde_json::to_value(value).map_err(|_| Error::InternalInvariant)?;
    let value = value.as_str().ok_or(Error::InternalInvariant)?;
    Ok(format!("{V2}{category}:{value}"))
}

pub(super) fn optional_datetime(
    builder: &mut Builder,
    parent: &str,
    name: &str,
    value: Option<&str>,
) -> Result<()> {
    if let Some(value) = value {
        builder.datetime(parent, &field(name), value)?;
    }
    Ok(())
}

pub(super) fn list_text(
    builder: &mut Builder,
    parent: &str,
    name: &str,
    base: &str,
    values: &[String],
) -> Result<()> {
    list(builder, parent, name, base, values, false)
}

pub(super) fn list_iris(
    builder: &mut Builder,
    parent: &str,
    name: &str,
    base: &str,
    values: &[String],
) -> Result<()> {
    list(builder, parent, name, base, values, true)
}

fn list(
    builder: &mut Builder,
    parent: &str,
    name: &str,
    base: &str,
    values: &[String],
    iris: bool,
) -> Result<()> {
    let nodes = structured_list(builder, parent, name, base, values.len(), "ListEntry")?;
    for (node, value) in nodes.iter().zip(values) {
        if iris {
            builder.iri(node, &field("iriValue"), value)?;
        } else {
            builder.text(node, &field("value"), value)?;
        }
    }
    Ok(())
}

pub(super) fn list_u32(
    builder: &mut Builder,
    parent: &str,
    name: &str,
    base: &str,
    values: &[u32],
) -> Result<()> {
    let nodes = structured_list(builder, parent, name, base, values.len(), "ListEntry")?;
    for (node, value) in nodes.iter().zip(values) {
        builder.integer(node, &field("integerValue"), *value as i64)?;
    }
    Ok(())
}

pub(super) fn structured_list(
    builder: &mut Builder,
    parent: &str,
    name: &str,
    base: &str,
    count: usize,
    class: &str,
) -> Result<Vec<String>> {
    let list = format!("{base}:list:{name}");
    builder.iri(parent, &field(name), &list)?;
    builder.iri(&list, RDF_TYPE, &format!("{V2}OrderedList"))?;
    if matches!(name, "targets" | "assets" | "targetEnvironments") {
        builder.iri(&list, RDF_TYPE, &format!("{V2}TargetList"))?;
    }
    if matches!(name, "sources" | "bindings" | "profiles") || name == "alternatives" && count > 0 {
        builder.iri(&list, RDF_TYPE, &format!("{V2}RequiredList"))?;
    }
    if matches!(
        name,
        "steps" | "assertions" | "observations" | "roles" | "evidenceMap"
    ) {
        builder.iri(&list, RDF_TYPE, &format!("{V2}RequiredList"))?;
    }
    builder.integer(&list, &field("itemCount"), count as i64)?;
    let mut nodes = Vec::with_capacity(count);
    for index in 0..count {
        let node = format!("{list}:entry:{}", index + 1);
        builder.iri(&list, &field("entry"), &node)?;
        builder.iri(&node, RDF_TYPE, &format!("{V2}{class}"))?;
        builder.iri(&node, &field("parent"), &list)?;
        builder.integer(&node, &field("ordinal"), (index + 1) as i64)?;
        nodes.push(node);
    }
    Ok(nodes)
}
