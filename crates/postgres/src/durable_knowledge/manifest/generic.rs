use super::super::*;
use crate::knowledge_lifecycle::rdf;
use sqlx::Row;

mod resource;

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
    let projection = super::inquiry::load(tx, tenant, workspace, run).await?;
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
        if projection
            .as_ref()
            .is_some_and(|value| !value.allows_binding(&row.binding_kind))
        {
            continue;
        }
        if projection
            .as_ref()
            .and_then(|value| value.stage())
            .is_some()
            && row.revision_contract.as_deref() != Some("dk-2")
        {
            continue;
        }
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
            if let Some(value) = projection.as_ref().filter(|value| value.stage().is_some()) {
                let selection = row
                    .event_payload
                    .as_ref()
                    .and_then(|payload| payload.pointer("/planned/document/planning_briefs"))
                    .filter(|briefs| briefs.is_array())
                    .map(|briefs| {
                        let briefs: Vec<PlanningBrief> = decode(briefs.clone())?;
                        Ok(value.select_briefs(&briefs, purpose))
                    })
                    .transpose()?;
                match selection {
                    Some(super::inquiry::BriefSelection::Full) => {
                        return Err(Error::InternalInvariant);
                    }
                    Some(super::inquiry::BriefSelection::Omit {
                        needs_context: false,
                    }) => continue,
                    Some(super::inquiry::BriefSelection::Omit {
                        needs_context: true,
                    }) => {
                        gaps.push("required_selector_context_missing".into());
                        continue;
                    }
                    Some(super::inquiry::BriefSelection::Briefs { needs_context, .. }) => {
                        if needs_context {
                            gaps.push("required_selector_context_missing".into());
                        }
                    }
                    None => continue,
                }
            }
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
        let (resource, valid_from, valid_until, review_due_at) = if row.revision_contract.as_deref()
            == Some("dk-2")
        {
            let value =
                resource::typed(tx, tenant, workspace, &row, projection.as_ref(), purpose).await?;
            if value.needs_context && blocking(purpose) {
                gaps.push("required_selector_context_missing".into());
            }
            let Some(resource) = value.resource else {
                continue;
            };
            (
                resource,
                value.valid_from,
                value.valid_until,
                value.review_due_at,
            )
        } else {
            (
                resource::legacy(tx, tenant, workspace, &row).await?,
                None,
                None,
                None,
            )
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
        let review = crate::knowledge_maintenance::current_unit_review_status(
            tx,
            tenant,
            workspace,
            principal,
            resource.unit_id,
            resource.revision,
        )
        .await?;
        if review.needs_review {
            if blocking(purpose) {
                gaps.push(format!("knowledge_needs_review:{}", resource.unit_id));
            } else {
                warnings.push(format!(
                    "optional_knowledge_needs_review:{}",
                    resource.unit_id
                ));
            }
        }
        if review_due {
            warnings.push(format!("review_due:{}", resource.unit_id));
        }
        selected.push(resource);
    }
    let base_semantic_digest = digest(&(
        &definition_version,
        &definition_digest,
        &method_requirements,
        &selected,
        &gaps,
        &warnings,
    ))?;
    let semantic_digest = if let Some(value) = projection.as_ref() {
        digest(&(&base_semantic_digest, &value.inquiry, value.policy))?
    } else {
        base_semantic_digest
    };
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
            inquiry: projection.as_ref().map(|value| value.inquiry.clone()),
            projection_policy: projection.as_ref().map(|value| value.policy),
            selected,
            unresolved_needs: gaps,
            freshness_warnings: warnings,
        },
    })
}
