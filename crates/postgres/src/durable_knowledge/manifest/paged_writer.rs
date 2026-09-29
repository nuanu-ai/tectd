//! Transactional paged capture for selected DK1 and DK2 resources.
use super::*;
mod pin;
use pin::pin_resource;

pub(crate) enum PagedCapture {
    Paged(Box<PagedCaptured>),
    Inline(Box<Captured>),
}

struct PinScope {
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
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
        return Ok(Some(PagedCapture::Inline(Box::new(captured))));
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
    let pin_scope = PinScope {
        tenant,
        workspace,
        principal,
    };
    for resource in &snapshot.manifest.selected {
        let (pin, _) = pin_resource(
            tx,
            &pin_scope,
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
    Ok(Some(PagedCapture::Paged(Box::new(PagedCaptured {
        manifest: header,
        pins,
        snapshot,
        principal,
    }))))
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
