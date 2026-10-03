use super::super::*;
use crate::knowledge_lifecycle::rdf;
use sqlx::Row;
use std::collections::BTreeSet;

mod resource;
mod selection;

use selection::{binding, blocking, covered_supersession, methods};

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
    snapshot_inner(
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
        shared_digest,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn snapshot_with_proofs(
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
    session: Uuid,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<Snapshot> {
    snapshot_inner(
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
        shared_digest,
        Some((principal, session, proofs)),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn snapshot_inner(
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
    mut proofs: Option<crate::knowledge_lifecycle::PublicationProofContext<'_>>,
) -> Result<Snapshot> {
    tect_application::request_diagnostics::measure("pg.manifest_snapshot", async {
    if let Some((proof_principal, session, proof_scope)) = proofs.as_ref() {
        proof_scope.require_identity(tenant, workspace, *proof_principal, *session)?;
        if *proof_principal != principal {
            return Err(Error::InternalInvariant);
        }
    }
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
    // Only accessible, active, definition-matched DK-2 resources enter proof
    // preloading. Relational selection is rebuilt on every snapshot.
    if let Some((_, _, proof_scope)) = proofs
        .as_mut()
        .filter(|(_, _, scope)| scope.eager_preload())
    {
        let typed_rows = rows
            .iter()
            .filter(|row| {
                let projection_allows = projection
                    .as_ref()
                    .is_none_or(|value| value.allows_binding(&row.binding_kind));
                let pin_matches = row.binding_kind != "slice_phase"
                    || (row.definition_kind.as_deref() == Some(definition.kind.as_str())
                        && row.definition_version.as_deref() == Some(&definition_version)
                        && row.definition_digest.as_deref() == Some(&definition_digest));
                let inaccessible = (row.head_access == "owners_only"
                    || row.revision_access.as_deref() == Some("owners_only"))
                    && !owner;
                projection_allows
                    && row.revision_contract.as_deref() == Some("dk-2")
                    && !inaccessible
                    && row.binding_active
                    && row.head_active
                    && !row.head_payload_erased
                    && row.revision_payload_erased == Some(false)
                    && row.lifecycle == "active"
                    && pin_matches
                    && row.event_id.is_some()
            })
            .collect::<Vec<_>>();
        let mut keys = typed_rows
            .iter()
            .map(|row| {
                Ok(crate::knowledge_lifecycle::PublicationProofKey {
                    unit_id: row.unit_id,
                    revision: row.revision,
                    event_id: row.event_id.ok_or(Error::InternalInvariant)?,
                    include_revision: true,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        proof_scope.preload(tx, &keys).await?;
        keys.clear();
        let mut requested = BTreeSet::new();
        for row in &typed_rows {
            let verified = proof_scope
                .verify(
                    tx,
                    crate::knowledge_lifecycle::PublicationProofKey {
                        unit_id: row.unit_id,
                        revision: row.revision,
                        event_id: row.event_id.ok_or(Error::InternalInvariant)?,
                        include_revision: true,
                    },
                )
                .await?;
            // Preserve scalar same-resource refusal order: creation proof,
            // selected revision digest, then latest revalidation proof.
            resource::verify_revision_digest(row.rdf_digest.as_deref(), &verified.rdf_digest)?;
            let Some(document) = verified.input.planned.document.as_ref() else {
                continue;
            };
            let purpose: KnowledgeBindingPurpose =
                decode(serde_json::Value::String(row.purpose.clone()))?;
            let selection = projection
                .as_ref()
                .map(|value| value.select(document, purpose))
                .unwrap_or(super::inquiry::BriefSelection::Full);
            if !matches!(selection, super::inquiry::BriefSelection::Omit { .. }) {
                requested.insert((row.unit_id, row.revision));
            }
        }
        if !requested.is_empty() {
            // Revalidation is read only for documents rendered by the current
            // projection, matching typed()'s early return for omitted resources.
            // Consumers still re-read metadata and check exact source pins.
            let units = requested.iter().map(|(unit, _)| *unit).collect::<Vec<_>>();
            let revisions = requested
                .iter()
                .map(|(_, revision)| *revision)
                .collect::<Vec<_>>();
            let latest: Vec<(Uuid, i64, Uuid)> = sqlx::query_as(
                "SELECT requested.unit_id,requested.revision,latest.id FROM unnest($3::uuid[],$4::bigint[]) AS requested(unit_id,revision) CROSS JOIN LATERAL (SELECT v.id FROM knowledge_validation_events v WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.unit_id=requested.unit_id AND v.unit_revision=requested.revision AND NOT v.payload_erased ORDER BY v.created_at DESC,v.id DESC LIMIT 1) latest",
            ).bind(tenant).bind(workspace).bind(&units).bind(&revisions).fetch_all(&mut **tx).await.map_err(storage_error)?;
            let mut seen = BTreeSet::new();
            for (unit, revision, event) in latest {
                if !requested.contains(&(unit, revision)) || !seen.insert((unit, revision)) {
                    return Err(Error::InternalInvariant);
                }
                keys.push(crate::knowledge_lifecycle::PublicationProofKey {
                    unit_id: unit,
                    revision,
                    event_id: event,
                    include_revision: false,
                });
            }
        }
        proof_scope.preload(tx, &keys).await?;
    }
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
        if !row.binding_active
            && covered_supersession(
                tx,
                tenant,
                workspace,
                &row,
                proofs
                    .as_mut()
                    .map(|(principal, session, scope)| (*principal, *session, &mut **scope)),
            )
            .await?
        {
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
        let (resource, valid_from, valid_until, review_due_at) =
            if row.revision_contract.as_deref() == Some("dk-2") {
                let value = match proofs.as_mut() {
                    Some((proof_principal, session, scope)) => {
                        resource::typed_with_proofs(
                            tx,
                            tenant,
                            workspace,
                            &row,
                            projection.as_ref(),
                            purpose,
                            (*proof_principal, *session, &mut **scope),
                        )
                        .await?
                    }
                    None => {
                        resource::typed(tx, tenant, workspace, &row, projection.as_ref(), purpose)
                            .await?
                    }
                };
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
        let review = match proofs.as_mut() {
            Some((_, session, scope)) => {
                crate::knowledge_maintenance::current_unit_review_status_with_proofs(
                    tx,
                    tenant,
                    workspace,
                    principal,
                    resource.unit_id,
                    resource.revision,
                    *session,
                    scope,
                )
                .await?
            }
            None => {
                crate::knowledge_maintenance::current_unit_review_status(
                    tx,
                    tenant,
                    workspace,
                    principal,
                    resource.unit_id,
                    resource.revision,
                )
                .await?
            }
        };
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
    }).await
}
