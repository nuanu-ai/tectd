use super::super::*;
use crate::knowledge_lifecycle::rdf;
use sqlx::Row;

#[derive(Clone)]
pub(super) struct Snapshot {
    pub manifest: PipelineKnowledgeResourceManifest,
    pub blocking_gaps: Vec<String>,
}

struct BindingRow {
    binding_id: Uuid,
    unit_id: Uuid,
    revision: i64,
    binding_active: bool,
    head_active: bool,
    head_payload_erased: bool,
    binding_kind: String,
    purpose: String,
    version_resolution: String,
    program_id: Option<Uuid>,
    scope_id: Option<Uuid>,
    slice_id: Option<Uuid>,
    phase_id: Option<String>,
    definition_kind: Option<String>,
    definition_version: Option<String>,
    definition_digest: Option<String>,
    revision_contract: Option<String>,
    revision_access: Option<String>,
    revision_payload_erased: Option<bool>,
    event_id: Option<Uuid>,
    event_payload: Option<serde_json::Value>,
    rdf_digest: Option<String>,
    lifecycle: String,
    head_access: String,
}

fn binding(row: &BindingRow) -> Result<PipelineKnowledgeBindingPin> {
    let target = match row.binding_kind.as_str() {
        "workspace" => KnowledgeBindingTarget::Workspace,
        "program" => KnowledgeBindingTarget::Program {
            program_id: row.program_id.ok_or(Error::InternalInvariant)?,
        },
        "scope" => KnowledgeBindingTarget::Scope {
            scope_id: row.scope_id.ok_or(Error::InternalInvariant)?,
        },
        "slice" => KnowledgeBindingTarget::Slice {
            scope_id: row.scope_id.ok_or(Error::InternalInvariant)?,
            slice_id: row.slice_id.ok_or(Error::InternalInvariant)?,
        },
        "slice_phase" => KnowledgeBindingTarget::SlicePhase {
            scope_id: row.scope_id.ok_or(Error::InternalInvariant)?,
            slice_id: row.slice_id.ok_or(Error::InternalInvariant)?,
            phase_id: row.phase_id.clone().ok_or(Error::InternalInvariant)?,
        },
        _ => return Err(Error::InternalInvariant),
    };
    Ok(PipelineKnowledgeBindingPin {
        binding_iri: format!("urn:tect:dk:binding:{}", row.binding_id),
        target,
        purpose: decode(serde_json::Value::String(row.purpose.clone()))?,
        version_resolution: if row.version_resolution == "pinned_revision" {
            KnowledgeBindingVersion::PinnedRevision {
                revision: row.revision,
            }
        } else {
            KnowledgeBindingVersion::CurrentAccepted
        },
        definition_kind: row.definition_kind.clone(),
        definition_version: row.definition_version.clone(),
        definition_digest: row.definition_digest.clone(),
    })
}

fn blocking(purpose: KnowledgeBindingPurpose) -> bool {
    !matches!(purpose, KnowledgeBindingPurpose::Reference)
}

async fn covered_supersession(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: &BindingRow,
) -> Result<bool> {
    if row.version_resolution != "current_accepted" {
        return Ok(false);
    }
    let candidates: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT sup.event_id,sup.successor_unit_id FROM knowledge_supersessions sup WHERE sup.tenant_id=$1 AND sup.workspace_id=$2 AND sup.predecessor_unit_id=$3 AND $4=ANY(sup.replacement_binding_ids) AND EXISTS(SELECT 1 FROM knowledge_bindings replacement JOIN knowledge_unit_heads successor ON successor.tenant_id=replacement.tenant_id AND successor.workspace_id=replacement.workspace_id AND successor.unit_id=replacement.unit_id WHERE replacement.tenant_id=sup.tenant_id AND replacement.workspace_id=sup.workspace_id AND replacement.unit_id=sup.successor_unit_id AND replacement.active AND replacement.revision=successor.accepted_revision AND replacement.binding_kind=$5 AND replacement.program_id IS NOT DISTINCT FROM $6 AND replacement.scope_id IS NOT DISTINCT FROM $7 AND replacement.slice_id IS NOT DISTINCT FROM $8 AND replacement.phase_id IS NOT DISTINCT FROM $9 AND replacement.purpose=$10 AND replacement.version_resolution='current_accepted' AND successor.active AND NOT successor.payload_erased)",
    )
    .bind(tenant).bind(workspace).bind(row.unit_id).bind(row.binding_id)
    .bind(&row.binding_kind).bind(row.program_id).bind(row.scope_id).bind(row.slice_id)
    .bind(&row.phase_id).bind(&row.purpose)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected = KnowledgeDocumentBinding {
        target: binding(row)?.target,
        purpose: decode(serde_json::Value::String(row.purpose.clone()))?,
        version_resolution: KnowledgeBindingVersion::CurrentAccepted,
    };
    for (event, successor) in candidates {
        let verified = crate::knowledge_lifecycle::verify_publication_event(
            tx,
            tenant,
            workspace,
            row.unit_id,
            row.revision,
            event,
            false,
        )
        .await?;
        if verified.input.successor_unit == Some(successor)
            && verified.input.planned.operation == KnowledgeLifecycleOperation::Supersede
            && verified
                .input
                .planned
                .replacement_bindings
                .contains(&expected)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn methods(
    definition: &PipelineDefinitionSnapshot,
    phase: &str,
) -> Result<Vec<KnowledgeContractRef>> {
    let phase = definition
        .phases
        .iter()
        .find(|value| value.id == phase)
        .ok_or(Error::InternalInvariant)?;
    let mut values =
        phase
            .instructions
            .iter()
            .chain(&phase.skills)
            .chain(&phase.resources)
            .map(|value| KnowledgeContractRef {
                id: value.id.clone(),
                version: value.version.clone(),
                digest: value.digest.clone(),
                source_ref: value.origin_refs.first().cloned().unwrap_or_else(|| {
                    format!("embedded:{}:{}", definition.kind.as_str(), phase.id)
                }),
            })
            .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    Ok(values)
}

async fn latest_validation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
) -> Result<Option<PipelineKnowledgeValidationPin>> {
    let row: Option<(Uuid, i64)> = sqlx::query_as(
        "SELECT v.id,(SELECT count(*) FROM knowledge_validation_events x WHERE x.tenant_id=v.tenant_id AND x.workspace_id=v.workspace_id AND x.unit_id=v.unit_id AND x.unit_revision=v.unit_revision AND NOT x.payload_erased AND (x.created_at,x.id)<=(v.created_at,v.id)) FROM knowledge_validation_events v WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.unit_id=$3 AND v.unit_revision=$4 AND NOT v.payload_erased ORDER BY v.created_at DESC,v.id DESC LIMIT 1",
    ).bind(tenant).bind(workspace).bind(unit).bind(revision).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((event_id, sequence)) = row else {
        return Ok(None);
    };
    let verified = crate::knowledge_lifecycle::verify_publication_event(
        tx, tenant, workspace, unit, revision, event_id, false,
    )
    .await?;
    if verified.input.planned.operation != KnowledgeLifecycleOperation::Revalidate {
        return Err(Error::InternalInvariant);
    }
    let revalidation = verified
        .input
        .planned
        .revalidation
        .as_ref()
        .ok_or(Error::InternalInvariant)?;
    let source_pin_digest = digest(&verified.input.resolved_sources)?;
    let exact: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_validation_events WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND unit_id=$4 AND unit_revision=$5 AND NOT payload_erased AND sources=$6 AND source_pin_digest=$7 AND evidence_basis=$8 AND valid_until IS NOT DISTINCT FROM $9::timestamptz AND review_due_at IS NOT DISTINCT FROM $10::timestamptz)")
        .bind(tenant).bind(workspace).bind(event_id).bind(unit).bind(revision)
        .bind(json(&revalidation.sources)?).bind(source_pin_digest).bind(&revalidation.evidence_basis)
        .bind(&revalidation.valid_until).bind(&revalidation.review_due_at)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !exact {
        return Err(Error::InternalInvariant);
    }
    Ok(Some(PipelineKnowledgeValidationPin {
        event_id,
        event_iri: format!("urn:tect:dk:event:{tenant}:{workspace}:{event_id}"),
        event_digest: verified.rdf_digest,
        sequence,
        valid_until: revalidation.valid_until.clone(),
        review_due_at: revalidation.review_due_at.clone(),
        source_pins: verified
            .input
            .resolved_sources
            .into_iter()
            .map(|value| PipelineKnowledgeSourcePin {
                source_iri: value.pin.source_iri,
                digest: value.pin.digest,
                evidence_kind: value.pin.evidence_kind,
                observed_at: value.pin.observed_at,
                evidence_scope: value.pin.evidence_scope,
                title: value.title,
                uri: value.uri,
            })
            .collect(),
    }))
}

async fn typed_resource(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: &BindingRow,
) -> Result<PipelineKnowledgeResource> {
    let event_id = row.event_id.ok_or(Error::InternalInvariant)?;
    let verified = crate::knowledge_lifecycle::verify_publication_event(
        tx,
        tenant,
        workspace,
        row.unit_id,
        row.revision,
        event_id,
        true,
    )
    .await?;
    if row.rdf_digest.as_deref() != Some(verified.rdf_digest.as_str()) {
        return Err(Error::InternalInvariant);
    }
    let input = verified.input;
    let document = input
        .planned
        .document
        .as_ref()
        .ok_or(Error::InternalInvariant)?;
    if input.planned.unit_id != row.unit_id || input.content_revision != row.revision {
        return Err(Error::InternalInvariant);
    }
    let expected = rdf::build(&input)?;
    let latest = latest_validation(tx, tenant, workspace, row.unit_id, row.revision).await?;
    Ok(PipelineKnowledgeResource {
        unit_id: row.unit_id,
        revision: row.revision,
        lifecycle: decode(serde_json::Value::String(row.lifecycle.clone()))?,
        access_scope: decode(serde_json::Value::String(row.head_access.clone()))?,
        rdf_digest: row.rdf_digest.clone().ok_or(Error::InternalInvariant)?,
        unit_iri: expected.refs.unit,
        revision_iri: expected.refs.revision,
        title: document.title.clone(),
        canonical_text: document.canonical_text.clone(),
        knowledge_kind: document.knowledge_kind,
        epistemic_state: document.epistemic_state,
        target_iris: document.target_iris.clone(),
        profiles: document.profiles.clone(),
        conditions: document.conditions.clone(),
        exceptions: document.exceptions.clone(),
        sections: document.sections.clone(),
        source_pins: input
            .resolved_sources
            .into_iter()
            .map(|value| PipelineKnowledgeSourcePin {
                source_iri: value.pin.source_iri,
                digest: value.pin.digest,
                evidence_kind: value.pin.evidence_kind,
                observed_at: value.pin.observed_at,
                evidence_scope: value.pin.evidence_scope,
                title: value.title,
                uri: value.uri,
            })
            .collect(),
        latest_validation: latest,
        binding: binding(row)?,
        why_included: format!("{}_binding", row.binding_kind),
    })
}

async fn legacy_resource(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: &BindingRow,
) -> Result<PipelineKnowledgeResource> {
    let value =
        context::load_revision(tx, tenant, workspace, row.unit_id, Some(row.revision), true)
            .await?
            .ok_or(Error::InternalInvariant)?;
    let target = value.constraint.target_iri.clone();
    Ok(PipelineKnowledgeResource {
        unit_id: value.unit_id,
        revision: value.revision,
        lifecycle: if value.active {
            KnowledgeLifecycleState::Active
        } else {
            KnowledgeLifecycleState::Retracted
        },
        access_scope: decode(serde_json::Value::String(row.head_access.clone()))?,
        rdf_digest: value.rdf_digest,
        unit_iri: value.unit_iri,
        revision_iri: value.revision_iri,
        title: value.constraint.title.clone(),
        canonical_text: value.constraint.statement.clone(),
        knowledge_kind: KnowledgeKind::Constraint,
        epistemic_state: KnowledgeEpistemicState::Normative,
        target_iris: vec![target.clone()],
        profiles: vec![KnowledgeProfileId::General],
        conditions: value.constraint.conditions.clone(),
        exceptions: value.constraint.exceptions.clone(),
        sections: KnowledgeProfileSections {
            constraint: Some(KnowledgeConstraintSection {
                modality: value.constraint.modality,
                action: value.constraint.action.clone(),
                target_iri: target,
            }),
            ..Default::default()
        },
        source_pins: vec![PipelineKnowledgeSourcePin {
            source_iri: value.source_iri,
            digest: value.source_sha256,
            evidence_kind: KnowledgeEvidenceKind::Document,
            observed_at: None,
            evidence_scope: "legacy_dk1_revision".into(),
            title: value.constraint.source.title,
            uri: value.constraint.source.uri,
        }],
        latest_validation: None,
        binding: binding(row)?,
        why_included: format!("{}_binding", row.binding_kind),
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn snapshot(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
    run_revision: i64,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    id: Uuid,
    shared_digest: String,
) -> Result<Snapshot> {
    super::delivery::require_identity_ready(tx).await?;
    let (generation, definition_version, definition_digest, definition): (i64, String, String, serde_json::Value) = sqlx::query_as(
        "SELECT k.generation,r.definition_version,r.definition_digest,r.definition FROM workspace_knowledge_state k JOIN slice_pipeline_runs r ON r.tenant_id=k.tenant_id AND r.workspace_id=k.workspace_id WHERE k.tenant_id=$1 AND k.workspace_id=$2 AND r.id=$3 AND r.scope_id=$4 AND r.slice_id=$5",
    ).bind(tenant).bind(workspace).bind(run).bind(scope).bind(slice).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let definition: PipelineDefinitionSnapshot = decode(definition)?;
    let method_requirements = methods(&definition, phase)?;
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let rows = sqlx::query(
        "SELECT b.id,h.unit_id,CASE b.version_resolution WHEN 'pinned_revision' THEN b.pinned_revision ELSE h.accepted_revision END,b.active,h.active,h.payload_erased,b.binding_kind,b.purpose,b.version_resolution,b.program_id,b.scope_id,b.slice_id,b.phase_id,b.definition_kind,b.definition_version,b.definition_digest,r.contract_version,r.access_scope,r.payload_erased,r.publication_event_id,e.event_payload,r.rdf_digest,h.lifecycle,h.access_scope FROM knowledge_bindings b JOIN knowledge_unit_heads h ON h.tenant_id=b.tenant_id AND h.workspace_id=b.workspace_id AND h.unit_id=b.unit_id JOIN slice_pipeline_runs run ON run.tenant_id=b.tenant_id AND run.workspace_id=b.workspace_id AND run.id=$3 JOIN native_scopes ns ON ns.tenant_id=run.tenant_id AND ns.workspace_id=run.workspace_id AND ns.id=$4 JOIN scope_candidate_sets sc ON sc.tenant_id=ns.tenant_id AND sc.workspace_id=ns.workspace_id AND sc.id=ns.source_candidate_set_id LEFT JOIN knowledge_revisions r ON r.tenant_id=b.tenant_id AND r.workspace_id=b.workspace_id AND r.unit_id=b.unit_id AND r.revision=CASE b.version_resolution WHEN 'pinned_revision' THEN b.pinned_revision ELSE h.accepted_revision END LEFT JOIN knowledge_publication_events e ON e.tenant_id=r.tenant_id AND e.workspace_id=r.workspace_id AND e.id=r.publication_event_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.revision=h.accepted_revision AND (b.binding_kind='workspace' OR (b.binding_kind='program' AND b.program_id=sc.program_id) OR (b.binding_kind='scope' AND b.scope_id=$4) OR (b.binding_kind='slice' AND b.scope_id=$4 AND b.slice_id=$5) OR (b.binding_kind='slice_phase' AND b.scope_id=$4 AND b.slice_id=$5 AND b.phase_id=$6)) ORDER BY b.id",
    ).bind(tenant).bind(workspace).bind(run).bind(scope).bind(slice).bind(phase).fetch_all(&mut **tx).await.map_err(storage_error)?
        .into_iter().map(|row| Ok(BindingRow {
            binding_id: row.try_get(0).map_err(storage_error)?, unit_id: row.try_get(1).map_err(storage_error)?, revision: row.try_get(2).map_err(storage_error)?,
            binding_active: row.try_get(3).map_err(storage_error)?, head_active: row.try_get(4).map_err(storage_error)?, head_payload_erased: row.try_get(5).map_err(storage_error)?,
            binding_kind: row.try_get(6).map_err(storage_error)?, purpose: row.try_get(7).map_err(storage_error)?, version_resolution: row.try_get(8).map_err(storage_error)?,
            program_id: row.try_get(9).map_err(storage_error)?, scope_id: row.try_get(10).map_err(storage_error)?, slice_id: row.try_get(11).map_err(storage_error)?, phase_id: row.try_get(12).map_err(storage_error)?,
            definition_kind: row.try_get(13).map_err(storage_error)?, definition_version: row.try_get(14).map_err(storage_error)?, definition_digest: row.try_get(15).map_err(storage_error)?,
            revision_contract: row.try_get(16).map_err(storage_error)?, revision_access: row.try_get(17).map_err(storage_error)?, revision_payload_erased: row.try_get(18).map_err(storage_error)?, event_id: row.try_get(19).map_err(storage_error)?,
            event_payload: row.try_get(20).map_err(storage_error)?, rdf_digest: row.try_get(21).map_err(storage_error)?, lifecycle: row.try_get(22).map_err(storage_error)?, head_access: row.try_get(23).map_err(storage_error)?,
        })).collect::<Result<Vec<_>>>()?;
    let mut selected = Vec::new();
    let mut gaps = Vec::new();
    let mut warnings = Vec::new();
    for row in rows {
        if !row.binding_active && covered_supersession(tx, tenant, workspace, &row).await? {
            continue;
        }
        let purpose: KnowledgeBindingPurpose =
            decode(serde_json::Value::String(row.purpose.clone()))?;
        let pin_matches = row.binding_kind != "slice_phase"
            || (row.definition_kind.as_deref() == Some(definition.kind.as_str())
                && row.definition_version.as_deref() == Some(&definition_version)
                && row.definition_digest.as_deref() == Some(&definition_digest));
        let inaccessible = (row.head_access == "owners_only"
            || row.revision_access.as_deref() == Some("owners_only"))
            && !owner;
        let available = row.binding_active
            && row.head_active
            && !row.head_payload_erased
            && row.revision_payload_erased == Some(false)
            && row.lifecycle == "active"
            && pin_matches
            && row.event_id.is_some();
        if inaccessible || !available {
            let revision_missing = row.revision_contract.is_none();
            let reason = if inaccessible {
                "resource_inaccessible"
            } else if revision_missing {
                "resource_revision_missing"
            } else if !pin_matches {
                "binding_definition_changed"
            } else {
                "resource_unavailable"
            };
            if blocking(purpose) {
                gaps.push(reason.into());
            } else {
                warnings.push(format!("optional_{reason}"));
            }
            continue;
        }
        let resource = if row.revision_contract.as_deref() == Some("dk-2") {
            typed_resource(tx, tenant, workspace, &row).await?
        } else {
            legacy_resource(tx, tenant, workspace, &row).await?
        };
        let (valid_from, valid_until, review_due_at) =
            if row.revision_contract.as_deref() == Some("dk-2") {
                let input: rdf::RdfPublicationInput =
                    decode(row.event_payload.clone().ok_or(Error::InternalInvariant)?)?;
                let document = input.planned.document.ok_or(Error::InternalInvariant)?;
                (
                    document.valid_from,
                    resource
                        .latest_validation
                        .as_ref()
                        .and_then(|v| v.valid_until.clone())
                        .or(document.valid_until),
                    resource
                        .latest_validation
                        .as_ref()
                        .and_then(|v| v.review_due_at.clone())
                        .or(document.review_due_at),
                )
            } else {
                (None, None, None)
            };
        let (valid, review_due): (bool, bool) = sqlx::query_as("SELECT ($1::timestamptz IS NULL OR $1::timestamptz<=pg_catalog.clock_timestamp()) AND ($2::timestamptz IS NULL OR $2::timestamptz>=pg_catalog.clock_timestamp()),($3::timestamptz IS NOT NULL AND $3::timestamptz<pg_catalog.clock_timestamp())")
            .bind(valid_from).bind(valid_until).bind(review_due_at).fetch_one(&mut **tx).await.map_err(storage_error)?;
        if !valid {
            if blocking(purpose) {
                gaps.push("resource_expired".into());
            } else {
                warnings.push("optional_resource_expired".into());
            }
            continue;
        }
        if review_due {
            warnings.push(format!("review_due:{}", resource.unit_id));
        }
        selected.push(resource);
    }
    let semantic_digest = digest(&(
        &definition_version,
        &definition_digest,
        &method_requirements,
        &selected,
        &gaps,
        &warnings,
    ))?;
    Ok(Snapshot {
        blocking_gaps: gaps.clone(),
        manifest: PipelineKnowledgeResourceManifest {
            id,
            digest: shared_digest,
            semantic_digest,
            workspace_generation: generation,
            run_id: run,
            run_revision,
            phase_id: phase.into(),
            definition_version,
            definition_digest,
            method_requirements,
            selected,
            unresolved_needs: gaps,
            freshness_warnings: warnings,
        },
    })
}
