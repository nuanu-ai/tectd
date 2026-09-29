use super::*;
use sqlx::Row;

/// The serialized bytes are the committed resource-json-v1 representation.
/// No current binding, revision, or validation event is used for content.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn read_pinned_resource(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest_id: Uuid,
    manifest_digest: &str,
    ordinal: i64,
) -> Result<(
    PagedPipelineKnowledgeResourcePin,
    PipelineKnowledgeResource,
    Vec<u8>,
)> {
    require_consistent_snapshot(tx).await?;
    if ordinal < 0
        || delivery::authorize_manifest(tx, tenant, workspace, manifest_id, principal)
            .await?
            .as_deref()
            != Some(PAGED_KNOWLEDGE_CONTRACT_VERSION)
    {
        return Err(Error::NotFound);
    }
    let (manifest, pins) =
        load_manifest_commitment(tx, tenant, workspace, manifest_id, manifest_digest).await?;
    if ordinal >= manifest.resource_count {
        return Err(Error::InvalidArguments);
    }
    let pin = pins
        .into_iter()
        .nth(ordinal as usize)
        .ok_or(Error::InternalInvariant)?;
    pin.validate()?;
    let binding_matches: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_bindings WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND unit_id=$4)")
        .bind(tenant).bind(workspace).bind(pin.binding_id).bind(pin.unit_id)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !binding_matches
        || pin.binding_pin.binding_iri != format!("urn:tect:dk:binding:{}", pin.binding_id)
    {
        return Err(Error::InternalInvariant);
    }
    if pin.entry_kind == PagedKnowledgeEntryKind::Dk1Legacy {
        let revision =
            context::load_revision(tx, tenant, workspace, pin.unit_id, Some(pin.revision), true)
                .await?
                .ok_or(Error::InternalInvariant)?;
        let (resource, bytes) = reconstruct_legacy(&pin, revision)?;
        return Ok((pin, resource, bytes));
    }
    let event = pin.publication_event_id.ok_or(Error::InternalInvariant)?;
    let verified = crate::knowledge_lifecycle::verify_publication_event(
        tx,
        tenant,
        workspace,
        pin.unit_id,
        pin.revision,
        event,
        true,
    )
    .await?;
    if verified.rdf_digest != pin.rdf_digest {
        return Err(Error::InternalInvariant);
    }
    let latest_validation = if let Some(id) = pin.validation_event_id {
        let sequence: Option<i64> = sqlx::query_scalar("SELECT (SELECT count(*) FROM knowledge_validation_events x WHERE x.tenant_id=v.tenant_id AND x.workspace_id=v.workspace_id AND x.unit_id=v.unit_id AND x.unit_revision=v.unit_revision AND NOT x.payload_erased AND (x.created_at,x.id)<=(v.created_at,v.id)) FROM knowledge_validation_events v WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.id=$3 AND v.unit_id=$4 AND v.unit_revision=$5 AND NOT v.payload_erased")
            .bind(tenant).bind(workspace).bind(id).bind(pin.unit_id).bind(pin.revision)
            .fetch_optional(&mut **tx).await.map_err(storage_error)?;
        let validation = generic::resource::validation_at(
            tx,
            tenant,
            workspace,
            pin.unit_id,
            pin.revision,
            id,
            sequence.ok_or(Error::KnowledgePayloadErased)?,
        )
        .await?;
        if pin.validation_event_digest.as_deref() != Some(validation.event_digest.as_str()) {
            return Err(Error::InternalInvariant);
        }
        Some(validation)
    } else {
        None
    };
    let (resource, bytes) = reconstruct(&pin, verified, latest_validation)?;
    Ok((pin, resource, bytes))
}

pub(crate) async fn load_manifest_commitment(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest_id: Uuid,
    manifest_digest: &str,
) -> Result<(
    PagedPipelineKnowledgeManifest,
    Vec<PagedPipelineKnowledgeResourcePin>,
)> {
    let header = sqlx::query(
        "SELECT digest,semantic_digest,selected,unresolved_needs,resource_semantic_digest,workspace_generation,run_id,run_revision,phase_id,definition_version,definition_digest,method_requirements,resource_inquiry,resource_projection_policy,resource_unresolved_needs,freshness_warnings,resource_count,total_resource_bytes,resource_digest_algorithm FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND contract_version='dk-2-paged' AND NOT payload_erased",
    ).bind(tenant).bind(workspace).bind(manifest_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::KnowledgePayloadErased)?;
    let manifest = PagedPipelineKnowledgeManifest {
        contract_version: PAGED_KNOWLEDGE_CONTRACT_VERSION.into(),
        id: manifest_id,
        digest: header.try_get("digest").map_err(storage_error)?,
        semantic_digest: header
            .try_get("resource_semantic_digest")
            .map_err(storage_error)?,
        workspace_generation: header
            .try_get("workspace_generation")
            .map_err(storage_error)?,
        run_id: header.try_get("run_id").map_err(storage_error)?,
        run_revision: header.try_get("run_revision").map_err(storage_error)?,
        phase_id: header.try_get("phase_id").map_err(storage_error)?,
        definition_version: header
            .try_get("definition_version")
            .map_err(storage_error)?,
        definition_digest: header.try_get("definition_digest").map_err(storage_error)?,
        method_requirements: decode(
            header
                .try_get("method_requirements")
                .map_err(storage_error)?,
        )?,
        inquiry: header
            .try_get::<Option<serde_json::Value>, _>("resource_inquiry")
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        projection_policy: header
            .try_get::<Option<String>, _>("resource_projection_policy")
            .map_err(storage_error)?
            .map(|v| decode(serde_json::Value::String(v)))
            .transpose()?,
        unresolved_needs: decode(
            header
                .try_get("resource_unresolved_needs")
                .map_err(storage_error)?,
        )?,
        freshness_warnings: decode(
            header
                .try_get("freshness_warnings")
                .map_err(storage_error)?,
        )?,
        resource_count: header.try_get("resource_count").map_err(storage_error)?,
        total_resource_bytes: header
            .try_get("total_resource_bytes")
            .map_err(storage_error)?,
        resource_digest_algorithm: header
            .try_get("resource_digest_algorithm")
            .map_err(storage_error)?,
        page_route: "slice.pipeline.knowledge_page".into(),
    };
    if manifest.digest != manifest_digest {
        return Err(Error::InternalInvariant);
    }
    // Verify the entire ordered commitment before releasing one row. A deleted or
    // reordered child must never turn a page into an apparently complete response.
    let rows = sqlx::query("SELECT ordinal,entry_kind,unit_id,revision,publication_event_id,rdf_digest,binding_id,binding_pin,lifecycle,access_scope,validation_event_id,validation_event_digest,projection,resource_digest,resource_bytes FROM pipeline_knowledge_manifest_resources WHERE tenant_id=$1 AND workspace_id=$2 AND manifest_id=$3 ORDER BY ordinal")
        .bind(tenant).bind(workspace).bind(manifest_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut pins = Vec::with_capacity(rows.len());
    for row in &rows {
        let kind: String = row.try_get("entry_kind").map_err(storage_error)?;
        let pin = PagedPipelineKnowledgeResourcePin {
            ordinal: row.try_get("ordinal").map_err(storage_error)?,
            entry_kind: decode(serde_json::Value::String(kind))?,
            unit_id: row.try_get("unit_id").map_err(storage_error)?,
            revision: row.try_get("revision").map_err(storage_error)?,
            publication_event_id: row.try_get("publication_event_id").map_err(storage_error)?,
            rdf_digest: row.try_get("rdf_digest").map_err(storage_error)?,
            binding_id: row.try_get("binding_id").map_err(storage_error)?,
            binding_pin: decode(row.try_get("binding_pin").map_err(storage_error)?)?,
            lifecycle: decode(serde_json::Value::String(
                row.try_get("lifecycle").map_err(storage_error)?,
            ))?,
            access_scope: decode(serde_json::Value::String(
                row.try_get("access_scope").map_err(storage_error)?,
            ))?,
            validation_event_id: row.try_get("validation_event_id").map_err(storage_error)?,
            validation_event_digest: row
                .try_get("validation_event_digest")
                .map_err(storage_error)?,
            projection: decode(row.try_get("projection").map_err(storage_error)?)?,
            resource_digest: row.try_get("resource_digest").map_err(storage_error)?,
            resource_bytes: row.try_get("resource_bytes").map_err(storage_error)?,
        };
        pins.push(pin);
    }
    let legacy_selected: Vec<PipelineKnowledgeItem> =
        decode(header.try_get("selected").map_err(storage_error)?)?;
    let legacy_unresolved: Vec<String> =
        decode(header.try_get("unresolved_needs").map_err(storage_error)?)?;
    let legacy_semantic: String = header.try_get("semantic_digest").map_err(storage_error)?;
    if legacy::semantic(&legacy_selected, &legacy_unresolved)? != legacy_semantic
        || paged_manifest_digest(
            tenant,
            workspace,
            &manifest,
            &pins,
            &legacy_selected,
            &legacy_unresolved,
        )? != manifest.digest
    {
        return Err(Error::InternalInvariant);
    }
    Ok((manifest, pins))
}

pub(super) async fn verify_empty_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest_id: Uuid,
    manifest_digest: &str,
) -> Result<()> {
    require_consistent_snapshot(tx).await?;
    if delivery::authorize_manifest(tx, tenant, workspace, manifest_id, principal)
        .await?
        .as_deref()
        != Some(PAGED_KNOWLEDGE_CONTRACT_VERSION)
    {
        return Err(Error::NotFound);
    }
    let (manifest, pins) =
        load_manifest_commitment(tx, tenant, workspace, manifest_id, manifest_digest).await?;
    if manifest.resource_count != 0 || !pins.is_empty() {
        return Err(Error::InternalInvariant);
    }
    Ok(())
}

pub(super) fn reconstruct(
    pin: &PagedPipelineKnowledgeResourcePin,
    verified: crate::knowledge_lifecycle::VerifiedPublicationEvent,
    latest_validation: Option<PipelineKnowledgeValidationPin>,
) -> Result<(PipelineKnowledgeResource, Vec<u8>)> {
    let resource = assemble(pin, verified, latest_validation)?;
    let bytes = checked_resource_bytes(pin, &resource)?;
    Ok((resource, bytes))
}

pub(super) fn reconstruct_legacy(
    pin: &PagedPipelineKnowledgeResourcePin,
    revision: KnowledgeUnitRevision,
) -> Result<(PipelineKnowledgeResource, Vec<u8>)> {
    if pin.entry_kind != PagedKnowledgeEntryKind::Dk1Legacy
        || pin.publication_event_id.is_some()
        || pin.validation_event_id.is_some()
        || pin.validation_event_digest.is_some()
        || pin.projection.policy != PipelineKnowledgeProjectionPolicy::FullResources
        || !pin.projection.inquiry_briefs.is_empty()
        || revision.unit_id != pin.unit_id
        || revision.revision != pin.revision
        || revision.rdf_digest != pin.rdf_digest
    {
        return Err(Error::InternalInvariant);
    }
    let resource = generic::resource::legacy_from_revision(
        pin.lifecycle,
        revision,
        pin.access_scope,
        pin.binding_pin.clone(),
    );
    let bytes = checked_resource_bytes(pin, &resource)?;
    Ok((resource, bytes))
}

fn checked_resource_bytes(
    pin: &PagedPipelineKnowledgeResourcePin,
    resource: &PipelineKnowledgeResource,
) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(&resource).map_err(storage_error)?;
    if bytes.len() as i64 != pin.resource_bytes || sha256(&bytes) != pin.resource_digest {
        return Err(Error::InternalInvariant);
    }
    Ok(bytes)
}

pub(crate) fn assemble(
    pin: &PagedPipelineKnowledgeResourcePin,
    verified: crate::knowledge_lifecycle::VerifiedPublicationEvent,
    latest_validation: Option<PipelineKnowledgeValidationPin>,
) -> Result<PipelineKnowledgeResource> {
    if verified.rdf_digest != pin.rdf_digest
        || verified.input.planned.unit_id != pin.unit_id
        || verified.input.content_revision != pin.revision
        || Some(verified.input.event_id) != pin.publication_event_id
        || latest_validation.as_ref().map(|v| v.event_id) != pin.validation_event_id
        || latest_validation.as_ref().map(|v| v.event_digest.as_str())
            != pin.validation_event_digest.as_deref()
    {
        return Err(Error::InternalInvariant);
    }
    let document = verified
        .input
        .planned
        .document
        .as_ref()
        .ok_or(Error::InternalInvariant)?;
    let rdf = crate::knowledge_lifecycle::rdf::build(&verified.input)?;
    let (canonical_text, target_iris, conditions, exceptions, sections, inquiry_briefs) =
        if pin.projection.policy == PipelineKnowledgeProjectionPolicy::FullResources {
            (
                document.canonical_text.clone(),
                document.target_iris.clone(),
                document.conditions.clone(),
                document.exceptions.clone(),
                document.sections.clone(),
                None,
            )
        } else {
            if pin.projection.inquiry_briefs.is_empty() {
                return Err(Error::InternalInvariant);
            }
            let mut values = Vec::new();
            for brief_pin in &pin.projection.inquiry_briefs {
                let brief = document
                    .planning_briefs
                    .iter()
                    .find(|v| v.local_id == brief_pin.id)
                    .ok_or(Error::InternalInvariant)?;
                if digest(brief)? != brief_pin.digest {
                    return Err(Error::InternalInvariant);
                }
                values.push(brief.clone());
            }
            (
                inquiry::projected_text(&values),
                inquiry::projected_targets(&values),
                inquiry::projected_conditions(&values),
                inquiry::projected_exceptions(&values),
                KnowledgeProfileSections::default(),
                Some(values),
            )
        };
    let resource = PipelineKnowledgeResource {
        unit_id: pin.unit_id,
        revision: pin.revision,
        lifecycle: pin.lifecycle,
        access_scope: pin.access_scope,
        rdf_digest: pin.rdf_digest.clone(),
        unit_iri: rdf.refs.unit,
        revision_iri: rdf.refs.revision,
        title: document.title.clone(),
        canonical_text,
        knowledge_kind: document.knowledge_kind,
        epistemic_state: document.epistemic_state,
        target_iris,
        profiles: document.profiles.clone(),
        conditions,
        exceptions,
        sections,
        inquiry_briefs,
        source_pins: verified
            .input
            .resolved_sources
            .into_iter()
            .map(|v| PipelineKnowledgeSourcePin {
                source_iri: v.pin.source_iri,
                digest: v.pin.digest,
                evidence_kind: v.pin.evidence_kind,
                observed_at: v.pin.observed_at,
                evidence_scope: v.pin.evidence_scope,
                title: v.title,
                uri: v.uri,
            })
            .collect(),
        latest_validation,
        binding: pin.binding_pin.clone(),
        why_included: match pin.binding_pin.target {
            KnowledgeBindingTarget::Workspace => "workspace_binding",
            KnowledgeBindingTarget::Program { .. } => "program_binding",
            KnowledgeBindingTarget::Scope { .. } => "scope_binding",
            KnowledgeBindingTarget::Slice { .. } => "slice_binding",
            KnowledgeBindingTarget::SlicePhase { .. } => "slice_phase_binding",
        }
        .into(),
    };
    Ok(resource)
}
