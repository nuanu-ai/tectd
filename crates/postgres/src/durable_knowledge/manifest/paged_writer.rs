//! Transactional paged capture for selected DK1 and DK2 resources.
use super::*;
use sqlx::Row;
use sqlx::postgres::PgRow;

pub(crate) enum PagedCapture {
    Paged(PagedCaptured),
    Inline(Captured),
}

pub(crate) struct PagedCaptured {
    pub manifest: PagedPipelineKnowledgeManifest,
    pub pins: Vec<PagedPipelineKnowledgeResourcePin>,
    pub snapshot: generic::Snapshot,
    pub principal: Uuid,
}

fn needs_inline_capture(selected_count: usize, has_dk2_selected: bool) -> bool {
    selected_count == 0 || !has_dk2_selected
}

/// The caller owns the run transaction and workspace-knowledge-state lock. This
/// method does not write a run pointer or origin response by itself.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn capture_paged(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    run_revision: i64,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
) -> Result<Option<PagedCapture>> {
    let state: Option<(i64, bool)> = sqlx::query_as(
        "SELECT generation,capability_ready FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    ).bind(tenant).bind(workspace).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((generation, true)) = state else {
        return Ok(None);
    };
    delivery::require_identity_ready(tx).await?;
    let principal: Uuid =
        sqlx::query_scalar::<_, Option<Uuid>>("SELECT tect_dk_session_principal($1)")
            .bind(session)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?
            .ok_or(Error::Forbidden)?;
    let id = Uuid::new_v4();
    let snapshot = generic::snapshot(
        tx,
        tenant,
        workspace,
        principal,
        run,
        run_revision,
        scope,
        slice,
        phase,
        id,
        String::new(),
    )
    .await?;
    if snapshot.manifest.workspace_generation != generation {
        return Err(Error::InternalInvariant);
    }

    // DK1 keeps its existing inline knowledge projection and its original cap.
    // A DK2 resource is represented solely by a compact child pin.
    let (revisions, legacy_unresolved) = if inquiry::load(tx, tenant, workspace, run)
        .await?
        .as_ref()
        .and_then(|value| value.stage())
        .is_some()
    {
        (Vec::new(), Vec::new())
    } else {
        super::projection(tx, tenant, workspace, run, scope, slice, phase, principal).await?
    };
    if needs_inline_capture(snapshot.manifest.selected.len(), snapshot.has_dk2_selected) {
        // Keep the original inline form for an empty or DK1-only selection.
        let captured = super::capture_with_snapshot(
            tx,
            tenant,
            workspace,
            run,
            run_revision,
            scope,
            slice,
            phase,
            session,
        )
        .await?
        .ok_or(Error::InternalInvariant)?;
        return Ok(Some(PagedCapture::Inline(captured)));
    }
    for value in &revisions {
        let rows = rdf::native_rows(
            tx,
            tenant,
            workspace,
            value.unit_id,
            value.revision,
            value.publication_event_id,
        )
        .await?;
        rdf::validate_rows(&rows, value)?;
    }
    let legacy_items = super::items(&revisions);
    if serde_json::to_vec(&(&legacy_items, &legacy_unresolved))
        .map_err(storage_error)?
        .len()
        > DK_MAX_MANIFEST_BYTES
    {
        return Err(Error::CapacityExceeded);
    }
    // The snapshot already verified each DK2 publication and its native RDF
    // and receipt inside this capture transaction.
    let mut pins = Vec::with_capacity(snapshot.manifest.selected.len());
    for resource in &snapshot.manifest.selected {
        let (pin, _) = pin_resource(
            tx,
            tenant,
            workspace,
            principal,
            pins.len() as i64,
            resource,
            snapshot.manifest.projection_policy,
            &snapshot.publication_proofs,
        )
        .await?;
        pins.push(pin);
    }
    let legacy_semantic = legacy::semantic(&legacy_items, &legacy_unresolved)?;
    let total_resource_bytes = pins.iter().try_fold(0_i64, |sum, pin| {
        sum.checked_add(pin.resource_bytes)
            .ok_or(Error::CapacityExceeded)
    })?;
    let mut header = PagedPipelineKnowledgeManifest {
        contract_version: PAGED_KNOWLEDGE_CONTRACT_VERSION.into(),
        id,
        digest: "0".repeat(64),
        semantic_digest: snapshot.manifest.semantic_digest.clone(),
        workspace_generation: generation,
        run_id: run,
        run_revision,
        phase_id: phase.into(),
        definition_version: snapshot.manifest.definition_version.clone(),
        definition_digest: snapshot.manifest.definition_digest.clone(),
        method_requirements: snapshot.manifest.method_requirements.clone(),
        inquiry: snapshot.manifest.inquiry.clone(),
        projection_policy: snapshot.manifest.projection_policy,
        unresolved_needs: snapshot.manifest.unresolved_needs.clone(),
        freshness_warnings: snapshot.manifest.freshness_warnings.clone(),
        resource_count: pins.len() as i64,
        total_resource_bytes,
        resource_digest_algorithm: PAGED_RESOURCE_DIGEST_ALGORITHM.into(),
        page_route: "slice.pipeline.knowledge_page".into(),
    };
    header.digest = paged_manifest_digest(
        tenant,
        workspace,
        &header,
        &pins,
        &legacy_items,
        &legacy_unresolved,
    )?;
    sqlx::query("INSERT INTO pipeline_knowledge_manifests(id,tenant_id,workspace_id,run_id,run_revision,phase_id,workspace_generation,digest,semantic_digest,selected,unresolved_needs,contract_version,definition_version,definition_digest,method_requirements,selected_resources,resource_unresolved_needs,freshness_warnings,resource_semantic_digest,resource_inquiry,resource_projection_policy,resource_count,total_resource_bytes,resource_digest_algorithm) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'dk-2-paged',$12,$13,$14,NULL,$15,$16,$17,$18,$19,$20,$21,$22)")
        .bind(id).bind(tenant).bind(workspace).bind(run).bind(run_revision).bind(phase).bind(generation)
        .bind(&header.digest).bind(&legacy_semantic).bind(json(&legacy_items)?).bind(json(&legacy_unresolved)?)
        .bind(&header.definition_version).bind(&header.definition_digest).bind(json(&header.method_requirements)?)
        .bind(json(&header.unresolved_needs)?).bind(json(&header.freshness_warnings)?)
        .bind(&header.semantic_digest).bind(header.inquiry.as_ref().map(json).transpose()?)
        .bind(header.projection_policy.map(|v| serde_json::to_value(v).map_err(storage_error)).transpose()?.and_then(|v| v.as_str().map(str::to_owned)))
        .bind(header.resource_count).bind(header.total_resource_bytes).bind(&header.resource_digest_algorithm)
        .execute(&mut **tx).await.map_err(storage_error)?;
    for pin in &pins {
        sqlx::query("INSERT INTO pipeline_knowledge_manifest_resources(tenant_id,workspace_id,manifest_id,ordinal,entry_kind,unit_id,revision,publication_event_id,rdf_digest,binding_id,binding_pin,lifecycle,access_scope,validation_event_id,validation_event_digest,projection,resource_digest,resource_bytes) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)")
            .bind(tenant).bind(workspace).bind(id).bind(pin.ordinal)
            .bind(match pin.entry_kind { PagedKnowledgeEntryKind::Dk2Event => "dk2_event", PagedKnowledgeEntryKind::Dk1Legacy => "dk1_legacy" })
            .bind(pin.unit_id).bind(pin.revision).bind(pin.publication_event_id).bind(&pin.rdf_digest)
            .bind(pin.binding_id).bind(json(&pin.binding_pin)?)
            .bind(serde_json::to_value(pin.lifecycle).map_err(storage_error)?.as_str().ok_or(Error::InternalInvariant)?)
            .bind(serde_json::to_value(pin.access_scope).map_err(storage_error)?.as_str().ok_or(Error::InternalInvariant)?)
            .bind(pin.validation_event_id).bind(&pin.validation_event_digest).bind(json(&pin.projection)?)
            .bind(&pin.resource_digest).bind(pin.resource_bytes)
            .execute(&mut **tx).await.map_err(storage_error)?;
    }
    crate::knowledge_lifecycle::erase::register_pipeline_manifest_copies(tx, tenant, workspace, id)
        .await?;
    crate::knowledge_maintenance::register_manifest_consumers(tx, tenant, workspace, id).await?;
    Ok(Some(PagedCapture::Paged(PagedCaptured {
        manifest: header,
        pins,
        snapshot,
        principal,
    })))
}

async fn pin_resource(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    ordinal: i64,
    resource: &PipelineKnowledgeResource,
    projection_policy: Option<PipelineKnowledgeProjectionPolicy>,
    proofs: &std::collections::HashMap<
        (Uuid, i64, Uuid),
        crate::knowledge_lifecycle::VerifiedPublicationEvent,
    >,
) -> Result<(PagedPipelineKnowledgeResourcePin, bool)> {
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
        let reconstructed =
            super::paged_reader::assemble(&pin, verified, resource.latest_validation.clone())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durable_knowledge::manifest::unique_legacy_items;

    #[test]
    fn inline_fallback_selector_covers_empty_legacy_and_mixed() {
        assert!(needs_inline_capture(0, false)); // empty corpus
        assert!(needs_inline_capture(1, false)); // DK1 only
        assert!(!needs_inline_capture(2, true)); // selected DK1 and DK2
        assert!(!needs_inline_capture(1, true)); // inquiry-filtered DK1
    }

    #[test]
    fn mixed_capture_deduplicates_inline_revision_but_commits_both_bindings() {
        let tenant = Uuid::new_v4();
        let workspace = Uuid::new_v4();
        let unit = Uuid::new_v4();
        let hex = "a".repeat(64);
        let legacy = PipelineKnowledgeItem {
            unit_id: unit,
            revision: 1,
            rdf_digest: hex.clone(),
            source_sha256: hex.clone(),
            why_included: "workspace_binding".into(),
            unit_iri: "urn:test:unit".into(),
            revision_iri: "urn:test:revision".into(),
            source_iri: "urn:test:source".into(),
            source_uri: "urn:test:source-uri".into(),
            title: "shared legacy revision".into(),
            statement: "retained once inline".into(),
            modality: KnowledgeModality::Must,
            action: "retain".into(),
            target_iri: "urn:test:target".into(),
            conditions: vec![],
            exceptions: vec![],
        };
        let binding = |target| PipelineKnowledgeBindingPin {
            binding_iri: format!("urn:test:binding:{}", Uuid::new_v4()),
            target,
            purpose: KnowledgeBindingPurpose::Required,
            version_resolution: KnowledgeBindingVersion::CurrentAccepted,
            definition_kind: None,
            definition_version: None,
            definition_digest: None,
        };
        let pin = |ordinal, kind, unit_id, target| PagedPipelineKnowledgeResourcePin {
            ordinal,
            entry_kind: kind,
            unit_id,
            revision: 1,
            publication_event_id: (kind == PagedKnowledgeEntryKind::Dk2Event).then(Uuid::new_v4),
            rdf_digest: hex.clone(),
            binding_id: Uuid::new_v4(),
            binding_pin: binding(target),
            lifecycle: KnowledgeLifecycleState::Active,
            access_scope: KnowledgeAccessScope::WorkspaceMembers,
            validation_event_id: None,
            validation_event_digest: None,
            projection: PagedKnowledgeProjectionPin {
                policy: PipelineKnowledgeProjectionPolicy::FullResources,
                inquiry_briefs: vec![],
            },
            resource_digest: hex.clone(),
            resource_bytes: 17,
        };
        let rows = vec![
            pin(
                0,
                PagedKnowledgeEntryKind::Dk1Legacy,
                unit,
                KnowledgeBindingTarget::Workspace,
            ),
            pin(
                1,
                PagedKnowledgeEntryKind::Dk1Legacy,
                unit,
                KnowledgeBindingTarget::SlicePhase {
                    scope_id: Uuid::new_v4(),
                    slice_id: Uuid::new_v4(),
                    phase_id: "P01".into(),
                },
            ),
            pin(
                2,
                PagedKnowledgeEntryKind::Dk2Event,
                Uuid::new_v4(),
                KnowledgeBindingTarget::Workspace,
            ),
        ];
        let header = PagedPipelineKnowledgeManifest {
            contract_version: PAGED_KNOWLEDGE_CONTRACT_VERSION.into(),
            id: Uuid::new_v4(),
            digest: hex.clone(),
            semantic_digest: hex.clone(),
            workspace_generation: 1,
            run_id: Uuid::new_v4(),
            run_revision: 1,
            phase_id: "P01".into(),
            definition_version: "v1".into(),
            definition_digest: hex,
            method_requirements: vec![],
            inquiry: None,
            projection_policy: None,
            unresolved_needs: vec![],
            freshness_warnings: vec![],
            resource_count: rows.len() as i64,
            total_resource_bytes: 51,
            resource_digest_algorithm: PAGED_RESOURCE_DIGEST_ALGORITHM.into(),
            page_route: "slice.pipeline.knowledge_page".into(),
        };
        let selected = unique_legacy_items(vec![legacy.clone(), legacy.clone()]);
        assert_eq!(selected, vec![legacy.clone()]);
        let digest =
            paged_manifest_digest(tenant, workspace, &header, &rows, &selected, &[]).unwrap();
        assert_eq!(digest.len(), 64);
        assert_eq!(
            paged_manifest_digest(
                tenant,
                workspace,
                &header,
                &rows,
                &[legacy.clone(), legacy],
                &[],
            ),
            Err(Error::InternalInvariant)
        );
        let mut changed = rows.clone();
        changed[1].binding_pin.purpose = KnowledgeBindingPurpose::Reference;
        assert_ne!(
            paged_manifest_digest(tenant, workspace, &header, &changed, &selected, &[]).unwrap(),
            digest
        );
    }
}
