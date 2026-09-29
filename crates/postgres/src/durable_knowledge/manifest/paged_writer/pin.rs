use super::super::*;
use super::PinScope;
use sqlx::Row;
use sqlx::postgres::PgRow;

pub(super) async fn pin_resource(
    tx: &mut Transaction<'_, Postgres>,
    scope: &PinScope,
    ordinal: i64,
    resource: &PipelineKnowledgeResource,
    projection_policy: Option<PipelineKnowledgeProjectionPolicy>,
    proofs: &std::collections::HashMap<
        (Uuid, i64, Uuid),
        crate::knowledge_lifecycle::VerifiedPublicationEvent,
    >,
) -> Result<(PagedPipelineKnowledgeResourcePin, bool)> {
    let tenant = scope.tenant;
    let workspace = scope.workspace;
    let principal = scope.principal;
    let binding_id: Uuid = resource
        .binding
        .binding_iri
        .strip_prefix("urn:tect:dk:binding:")
        .ok_or(Error::InternalInvariant)?
        .parse()
        .map_err(|_| Error::InternalInvariant)?;
    let row = sqlx::query("SELECT b.unit_id,b.revision AS binding_revision,b.active AS binding_active,b.binding_kind,b.purpose,b.version_resolution,b.pinned_revision,b.program_id,b.scope_id,b.slice_id,b.phase_id,b.definition_kind,b.definition_version,b.definition_digest,h.accepted_revision,h.active AS head_active,h.payload_erased AS head_erased,h.lifecycle,h.access_scope AS head_access,r.contract_version,r.access_scope AS revision_access,r.payload_erased AS revision_erased,r.publication_event_id,r.rdf_digest FROM knowledge_bindings b JOIN knowledge_unit_heads h ON h.tenant_id=b.tenant_id AND h.workspace_id=b.workspace_id AND h.unit_id=b.unit_id JOIN knowledge_revisions r ON r.tenant_id=b.tenant_id AND r.workspace_id=b.workspace_id AND r.unit_id=b.unit_id AND r.revision=$4 WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.id=$3")
        .bind(tenant).bind(workspace).bind(binding_id).bind(resource.revision)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::InternalInvariant)?;
    let get_uuid = |key| row.try_get::<Uuid, _>(key).map_err(storage_error);
    let unit = get_uuid("unit_id")?;
    let accepted: i64 = row.try_get("accepted_revision").map_err(storage_error)?;
    let contract: String = row.try_get("contract_version").map_err(storage_error)?;
    let event: Option<Uuid> = row.try_get("publication_event_id").map_err(storage_error)?;
    let rdf: String = row.try_get("rdf_digest").map_err(storage_error)?;
    let lifecycle: String = row.try_get("lifecycle").map_err(storage_error)?;
    let access: String = row.try_get("head_access").map_err(storage_error)?;
    let rev_access: String = row.try_get("revision_access").map_err(storage_error)?;
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if unit != resource.unit_id
        || accepted
            != row
                .try_get::<i64, _>("binding_revision")
                .map_err(storage_error)?
        || !row
            .try_get::<bool, _>("binding_active")
            .map_err(storage_error)?
        || !row
            .try_get::<bool, _>("head_active")
            .map_err(storage_error)?
        || row
            .try_get::<bool, _>("head_erased")
            .map_err(storage_error)?
        || row
            .try_get::<bool, _>("revision_erased")
            .map_err(storage_error)?
        || lifecycle != "active"
        || ((access == "owners_only" || rev_access == "owners_only") && !owner)
        || rdf != resource.rdf_digest
        || resource.lifecycle != KnowledgeLifecycleState::Active
        || resource.access_scope != decode(serde_json::Value::String(access))?
        || !binding_matches(&row, &resource.binding, resource.revision)?
    {
        return Err(Error::InternalInvariant);
    }
    let is_legacy = contract == "dk-1";
    if !is_legacy && contract != "dk-2" {
        return Err(Error::InternalInvariant);
    }
    let bytes = serde_json::to_vec(resource).map_err(storage_error)?;
    let briefs = resource
        .inquiry_briefs
        .as_ref()
        .map(|values| {
            values
                .iter()
                .map(|v| {
                    Ok(PagedKnowledgeBriefPin {
                        id: v.local_id.clone(),
                        digest: digest(v)?,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .transpose()?
        .unwrap_or_default();
    let policy = if resource.inquiry_briefs.is_none() {
        PipelineKnowledgeProjectionPolicy::FullResources
    } else {
        projection_policy.ok_or(Error::InternalInvariant)?
    };
    let pin = PagedPipelineKnowledgeResourcePin {
        ordinal,
        entry_kind: if is_legacy {
            PagedKnowledgeEntryKind::Dk1Legacy
        } else {
            PagedKnowledgeEntryKind::Dk2Event
        },
        unit_id: resource.unit_id,
        revision: resource.revision,
        publication_event_id: if is_legacy { None } else { event },
        rdf_digest: rdf,
        binding_id,
        binding_pin: resource.binding.clone(),
        lifecycle: resource.lifecycle,
        access_scope: resource.access_scope,
        validation_event_id: resource.latest_validation.as_ref().map(|v| v.event_id),
        validation_event_digest: resource
            .latest_validation
            .as_ref()
            .map(|v| v.event_digest.clone()),
        projection: PagedKnowledgeProjectionPin {
            policy,
            inquiry_briefs: briefs,
        },
        resource_digest: sha256(&bytes),
        resource_bytes: bytes.len() as i64,
    };
    pin.validate()?;
    if is_legacy {
        let revision =
            context::load_revision(tx, tenant, workspace, pin.unit_id, Some(pin.revision), true)
                .await?
                .ok_or(Error::InternalInvariant)?;
        if revision.rdf_digest != pin.rdf_digest
            || resource.latest_validation.is_some()
            || pin.projection.policy != PipelineKnowledgeProjectionPolicy::FullResources
            || !pin.projection.inquiry_briefs.is_empty()
            || generic::resource::legacy_from_revision(
                pin.lifecycle,
                revision,
                pin.access_scope,
                pin.binding_pin.clone(),
            ) != *resource
        {
            return Err(Error::InternalInvariant);
        }
    } else {
        let key = (
            resource.unit_id,
            resource.revision,
            event.ok_or(Error::InternalInvariant)?,
        );
        let verified = proofs.get(&key).ok_or(Error::InternalInvariant)?.clone();
        let reconstructed = super::super::paged_reader::assemble(
            &pin,
            verified,
            resource.latest_validation.clone(),
        )?;
        if &reconstructed != resource {
            return Err(Error::InternalInvariant);
        }
    }
    Ok((pin, is_legacy))
}

fn binding_matches(row: &PgRow, pin: &PipelineKnowledgeBindingPin, revision: i64) -> Result<bool> {
    let kind: String = row.try_get("binding_kind").map_err(storage_error)?;
    let target = match kind.as_str() {
        "workspace" => KnowledgeBindingTarget::Workspace,
        "program" => KnowledgeBindingTarget::Program {
            program_id: row
                .try_get::<Option<Uuid>, _>("program_id")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?,
        },
        "scope" => KnowledgeBindingTarget::Scope {
            scope_id: row
                .try_get::<Option<Uuid>, _>("scope_id")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?,
        },
        "slice" => KnowledgeBindingTarget::Slice {
            scope_id: row
                .try_get::<Option<Uuid>, _>("scope_id")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?,
            slice_id: row
                .try_get::<Option<Uuid>, _>("slice_id")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?,
        },
        "slice_phase" => KnowledgeBindingTarget::SlicePhase {
            scope_id: row
                .try_get::<Option<Uuid>, _>("scope_id")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?,
            slice_id: row
                .try_get::<Option<Uuid>, _>("slice_id")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?,
            phase_id: row
                .try_get::<Option<String>, _>("phase_id")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?,
        },
        _ => return Err(Error::InternalInvariant),
    };
    let purpose: String = row.try_get("purpose").map_err(storage_error)?;
    let version: String = row.try_get("version_resolution").map_err(storage_error)?;
    let expected_version = match version.as_str() {
        "current_accepted" => KnowledgeBindingVersion::CurrentAccepted,
        "pinned_revision" => KnowledgeBindingVersion::PinnedRevision {
            revision: row
                .try_get::<Option<i64>, _>("pinned_revision")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?,
        },
        _ => return Err(Error::InternalInvariant),
    };
    if matches!(expected_version, KnowledgeBindingVersion::PinnedRevision { revision: pinned } if pinned != revision)
    {
        return Ok(false);
    }
    Ok(target == pin.target
        && decode::<KnowledgeBindingPurpose>(serde_json::Value::String(purpose))? == pin.purpose
        && expected_version == pin.version_resolution
        && row
            .try_get::<Option<String>, _>("definition_kind")
            .map_err(storage_error)?
            == pin.definition_kind
        && row
            .try_get::<Option<String>, _>("definition_version")
            .map_err(storage_error)?
            == pin.definition_version
        && row
            .try_get::<Option<String>, _>("definition_digest")
            .map_err(storage_error)?
            == pin.definition_digest)
}
