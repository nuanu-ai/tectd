use super::resource::{dk2, legacy};
use super::*;

#[derive(Debug, Clone)]
pub(super) struct SearchEdge {
    pub from: String,
    pub to: String,
    pub relation: KnowledgeSearchRelation,
    pub predicate_path: Vec<String>,
    pub binding: Option<KnowledgeGraphBindingQualifier>,
}

#[derive(Debug, Clone)]
pub(super) struct SearchResource {
    pub unit_id: Uuid,
    pub resource_iri: String,
    pub revision: i64,
    pub revision_iri: String,
    pub title: String,
    pub canonical_text: String,
    pub kind: KnowledgeKind,
    pub lifecycle: KnowledgeLifecycleState,
    pub access_scope: KnowledgeAccessScope,
    pub contract_version: String,
    pub verified_payload_bytes: usize,
    pub source_digests: Vec<String>,
    pub freshness_warnings: Vec<String>,
    pub edges: Vec<SearchEdge>,
    pub visible_internal_endpoints: Vec<String>,
}

struct BindingScope {
    level: i16,
    program: Option<Uuid>,
    scope: Option<Uuid>,
    slice: Option<Uuid>,
    phase: Option<String>,
}

pub(super) async fn validate_binding(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    target: Option<&KnowledgeBindingTarget>,
) -> Result<()> {
    binding_scope(tx, tenant, workspace, target)
        .await
        .map(|_| ())
}

async fn binding_scope(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    target: Option<&KnowledgeBindingTarget>,
) -> Result<BindingScope> {
    let Some(target) = target else {
        return Ok(BindingScope {
            level: 0,
            program: None,
            scope: None,
            slice: None,
            phase: None,
        });
    };
    match target {
        KnowledgeBindingTarget::Workspace => Ok(BindingScope {
            level: 0,
            program: None,
            scope: None,
            slice: None,
            phase: None,
        }),
        KnowledgeBindingTarget::Program { program_id } => {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM programs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)")
                .bind(tenant).bind(workspace).bind(program_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
            exists
                .then_some(BindingScope {
                    level: 1,
                    program: Some(*program_id),
                    scope: None,
                    slice: None,
                    phase: None,
                })
                .ok_or(Error::NotFound)
        }
        KnowledgeBindingTarget::Scope { scope_id } => {
            scope_row(tx, tenant, workspace, *scope_id, None, None).await
        }
        KnowledgeBindingTarget::Slice { scope_id, slice_id } => {
            scope_row(tx, tenant, workspace, *scope_id, Some(*slice_id), None).await
        }
        KnowledgeBindingTarget::SlicePhase {
            scope_id,
            slice_id,
            phase_id,
        } => {
            scope_row(
                tx,
                tenant,
                workspace,
                *scope_id,
                Some(*slice_id),
                Some(phase_id.clone()),
            )
            .await
        }
    }
}

async fn scope_row(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
    slice: Option<Uuid>,
    phase: Option<String>,
) -> Result<BindingScope> {
    let program: Option<Uuid> = sqlx::query_scalar("SELECT c.program_id FROM native_scopes s JOIN scope_candidate_sets c ON c.tenant_id=s.tenant_id AND c.workspace_id=s.workspace_id AND c.id=s.source_candidate_set_id WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3")
        .bind(tenant).bind(workspace).bind(scope).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let program = program.ok_or(Error::NotFound)?;
    if let Some(slice_id) = slice {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND scope_id=$4)")
            .bind(tenant).bind(workspace).bind(slice_id).bind(scope).fetch_one(&mut **tx).await.map_err(storage_error)?;
        if !exists {
            return Err(Error::NotFound);
        }
        if let Some(phase_id) = &phase {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM slice_pipeline_runs r CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(r.definition->'phases') p WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.slice_id=$3 AND p->>'id'=$4)")
                .bind(tenant).bind(workspace).bind(slice_id).bind(phase_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
            if !exists {
                return Err(Error::NotFound);
            }
        }
    }
    Ok(BindingScope {
        level: if phase.is_some() {
            4
        } else if slice.is_some() {
            3
        } else {
            2
        },
        program: Some(program),
        scope: Some(scope),
        slice,
        phase,
    })
}

pub(super) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    request: &KnowledgeSearchQuery,
) -> Result<(Vec<SearchResource>, bool, u64, bool)> {
    let scope = binding_scope(tx, tenant, workspace, request.binding.as_ref()).await?;
    let limit = i64::from(request.corpus_limit) + 1;
    let rows: Vec<(Uuid, String, i64, String, i64)> = sqlx::query_as(
        "SELECT h.unit_id,h.contract_version,h.accepted_revision,r.resource_iri,r.verified_payload_bytes FROM knowledge_unit_heads h JOIN knowledge_revisions rev ON rev.tenant_id=h.tenant_id AND rev.workspace_id=h.workspace_id AND rev.unit_id=h.unit_id AND rev.revision=h.accepted_revision JOIN knowledge_search_resources r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.lifecycle='active' AND h.active AND NOT h.payload_erased AND NOT rev.payload_erased AND NOT EXISTS(SELECT 1 FROM knowledge_suppression_ledger l WHERE l.tenant_id=h.tenant_id AND l.workspace_id=h.workspace_id AND l.unit_id=h.unit_id) AND ((h.access_scope='workspace_members' AND rev.access_scope='workspace_members' AND r.access_scope='workspace_members') OR tect_dk_is_owner($3)) AND ($4=0 OR EXISTS(SELECT 1 FROM knowledge_bindings b WHERE b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id AND b.unit_id=h.unit_id AND b.revision=h.accepted_revision AND b.active AND (b.binding_kind='workspace' OR ($5::uuid IS NOT NULL AND b.binding_kind='program' AND b.program_id=$5) OR ($5::uuid IS NOT NULL AND b.scope_id IS NOT NULL AND EXISTS(SELECT 1 FROM native_scopes ns JOIN scope_candidate_sets cs ON cs.tenant_id=ns.tenant_id AND cs.workspace_id=ns.workspace_id AND cs.id=ns.source_candidate_set_id WHERE ns.tenant_id=b.tenant_id AND ns.workspace_id=b.workspace_id AND ns.id=b.scope_id AND cs.program_id=$5) AND ($4=1 OR b.scope_id=$6) AND ($4<3 OR b.binding_kind='scope' OR b.slice_id=$7) AND ($4<4 OR b.binding_kind<>'slice_phase' OR b.phase_id=$8))))) AND (cardinality($9::text[])=0 OR r.knowledge_kind=ANY($9)) ORDER BY r.resource_iri LIMIT $10"
    ).bind(tenant).bind(workspace).bind(principal).bind(scope.level).bind(scope.program).bind(scope.scope).bind(scope.slice).bind(scope.phase).bind(request.kinds.iter().map(super::enum_text).collect::<Result<Vec<_>>>()?).bind(limit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let truncated = rows.len() > request.corpus_limit as usize;
    let mut resources = Vec::new();
    let mut bytes = 0u64;
    let mut byte_exhausted = false;
    for (unit, contract, revision, _, payload_bytes) in
        rows.into_iter().take(request.corpus_limit as usize)
    {
        let payload_bytes = u64::try_from(payload_bytes).map_err(|_| Error::InternalInvariant)?;
        if bytes.saturating_add(payload_bytes) > KNOWLEDGE_SEARCH_CORPUS_BYTE_BUDGET {
            byte_exhausted = true;
            break;
        }
        bytes += payload_bytes;
        if let Some(resource) = load_one(
            tx, tenant, workspace, principal, unit, revision, &contract, true,
        )
        .await?
        {
            if !projection_matches(tx, tenant, workspace, &resource).await? {
                return Err(Error::InternalInvariant);
            }
            resources.push(resource);
        }
    }
    Ok((
        resources,
        truncated || byte_exhausted,
        bytes,
        byte_exhausted,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn load_one(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    unit: Uuid,
    revision: i64,
    contract: &str,
    require_eligible: bool,
) -> Result<Option<SearchResource>> {
    if contract == "dk-1" {
        let Some(value) = crate::durable_knowledge::context::load_revision(
            tx,
            tenant,
            workspace,
            unit,
            Some(revision),
            true,
        )
        .await?
        else {
            return Ok(None);
        };
        if !value.active {
            return Ok(None);
        }
        let access:String=sqlx::query_scalar("SELECT CASE WHEN h.access_scope='owners_only' OR r.access_scope='owners_only' THEN 'owners_only' ELSE 'workspace_members' END FROM knowledge_unit_heads h JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=$4 WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).bind(revision).fetch_one(&mut **tx).await.map_err(storage_error)?;
        let access =
            serde_json::from_value(serde_json::Value::String(access)).map_err(storage_error)?;
        return Ok(Some(legacy(value, access)?));
    }
    if contract != "dk-2" {
        return Err(Error::InternalInvariant);
    }
    let response = match if require_eligible {
        crate::knowledge_lifecycle::eligible_unit(
            tx,
            tenant,
            workspace,
            principal,
            unit,
            Some(revision),
        )
        .await
    } else {
        crate::knowledge_lifecycle::unit(
            tx,
            tenant,
            workspace,
            principal,
            &KnowledgeUnitQuery {
                unit_id: unit,
                revision: Some(revision),
                fragment: None,
            },
        )
        .await?
        .ok_or(Error::NotFound)
    } {
        Ok(value) => value,
        Err(Error::NeedsContext) => return Ok(None),
        Err(error) => return Err(error),
    };
    let KnowledgeUnitResponse::Document(value) = response else {
        return Err(Error::InternalInvariant);
    };
    let event: Uuid = sqlx::query_scalar("SELECT publication_event_id FROM knowledge_revisions WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND revision=$4")
        .bind(tenant).bind(workspace).bind(unit).bind(revision).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let verified = crate::knowledge_lifecycle::verify_publication_event(
        tx, tenant, workspace, unit, revision, event, true,
    )
    .await?;
    let warnings = effective_review_warning(
        tx,
        tenant,
        workspace,
        principal,
        unit,
        revision,
        value.document.review_due_at.as_deref(),
    )
    .await?;
    Ok(Some(dk2(
        tenant,
        workspace,
        *value,
        verified.input.resolved_sources,
        warnings,
    )?))
}

async fn review_warning(
    tx: &mut Transaction<'_, Postgres>,
    value: Option<&str>,
) -> Result<Vec<String>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let due: bool = sqlx::query_scalar("SELECT $1::timestamptz < pg_catalog.clock_timestamp()")
        .bind(value)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    Ok(if due {
        vec!["review_due".into()]
    } else {
        Vec::new()
    })
}

async fn effective_review_warning(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    unit: Uuid,
    revision: i64,
    document_due: Option<&str>,
) -> Result<Vec<String>> {
    let event:Option<Uuid>=sqlx::query_scalar("SELECT id FROM knowledge_validation_events WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND unit_revision=$4 AND NOT payload_erased ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(tenant).bind(workspace).bind(unit).bind(revision).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let due = if let Some(event) = event {
        let verified = crate::knowledge_lifecycle::verify_publication_event(
            tx, tenant, workspace, unit, revision, event, false,
        )
        .await?;
        verified
            .input
            .planned
            .revalidation
            .and_then(|value| value.review_due_at)
    } else {
        document_due.map(String::from)
    };
    let mut warnings = review_warning(tx, due.as_deref()).await?;
    let status = crate::knowledge_maintenance::current_unit_review_status(
        tx, tenant, workspace, principal, unit, revision,
    )
    .await?;
    if !status.maintenance_bases.is_empty() {
        warnings.push("knowledge_needs_review".into());
    }
    warnings.sort();
    warnings.dedup();
    Ok(warnings)
}

async fn projection_matches(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    value: &SearchResource,
) -> Result<bool> {
    type ProjectionRow = (
        i64,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        serde_json::Value,
        String,
        i64,
    );
    let row:Option<ProjectionRow>=sqlx::query_as("SELECT revision,resource_iri,revision_iri,title,canonical_text,knowledge_kind,lifecycle,access_scope,source_digests,contract_version,verified_payload_bytes FROM knowledge_search_resources WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(value.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some(row) = row else { return Ok(false) };
    Ok(row.0 == value.revision
        && row.1 == value.resource_iri
        && row.2 == value.revision_iri
        && row.3 == value.title
        && row.4 == value.canonical_text
        && row.5 == enum_text(&value.kind)?
        && row.6 == enum_text(&value.lifecycle)?
        && row.7 == enum_text(&value.access_scope)?
        && row.8 == serde_json::to_value(&value.source_digests).map_err(storage_error)?
        && row.9 == value.contract_version
        && row.10
            == i64::try_from(value.verified_payload_bytes).map_err(|_| Error::CapacityExceeded)?)
}
