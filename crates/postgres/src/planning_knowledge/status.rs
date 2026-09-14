use super::*;

pub(super) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    stage: PlanningStage,
    owner: Uuid,
) -> Result<Option<PlanningKnowledgeManifest>> {
    let row = sqlx::query("SELECT id,owner_revision,input_revision,request_id,policy_id,policy_version,task_context_digest,task_context,workspace_generation,needs,selected,unresolved_needs,digest,payload_erased FROM planning_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND stage=$3 AND owner_id=$4 ORDER BY created_at DESC,id DESC LIMIT 1")
        .bind(tenant).bind(workspace).bind(stage_name(stage)).bind(owner).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    if row.try_get::<bool, _>(13).map_err(storage_error)? {
        return Err(Error::KnowledgePayloadErased);
    }
    let id: Uuid = row.try_get(0).map_err(storage_error)?;
    require_manifest_access(tx, tenant, workspace, principal, id).await?;
    Ok(Some(PlanningKnowledgeManifest {
        id,
        digest: row.try_get(12).map_err(storage_error)?,
        stage,
        owner_id: owner,
        owner_revision: row.try_get(1).map_err(storage_error)?,
        input_revision: row.try_get(2).map_err(storage_error)?,
        request_id: row.try_get(3).map_err(storage_error)?,
        policy_id: row.try_get(4).map_err(storage_error)?,
        policy_version: row.try_get(5).map_err(storage_error)?,
        task_context_digest: row.try_get(6).map_err(storage_error)?,
        task_context: decode(row.try_get(7).map_err(storage_error)?)?,
        workspace_generation: row.try_get(8).map_err(storage_error)?,
        needs: decode(row.try_get(9).map_err(storage_error)?)?,
        selected: decode(row.try_get(10).map_err(storage_error)?)?,
        unresolved_needs: decode(row.try_get(11).map_err(storage_error)?)?,
    }))
}

pub(crate) async fn status(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    stage: PlanningStage,
    owner: Uuid,
) -> Result<PlanningKnowledgeStatus> {
    let manifest = load(tx, tenant, workspace, principal, stage, owner).await?;
    let (stale_reasons, warnings) = match &manifest {
        Some(value) => stale_reasons_for_manifest(tx, tenant, workspace, principal, value).await?,
        None => (vec![], vec![]),
    };
    Ok(PlanningKnowledgeStatus {
        manifest,
        stale_reasons,
        warnings,
    })
}

pub(crate) async fn status_for_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    manifest: PlanningKnowledgeManifest,
) -> Result<PlanningKnowledgeStatus> {
    require_manifest_access(tx, tenant, workspace, principal, manifest.id).await?;
    let (stale_reasons, warnings) =
        stale_reasons_for_manifest(tx, tenant, workspace, principal, &manifest).await?;
    Ok(PlanningKnowledgeStatus {
        manifest: Some(manifest),
        stale_reasons,
        warnings,
    })
}

pub(super) async fn stale_reasons_for_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    value: &PlanningKnowledgeManifest,
) -> Result<(Vec<String>, Vec<String>)> {
    let current_generation:i64=sqlx::query_scalar("SELECT COALESCE((SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2),0)")
        .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let mut stale_reasons = Vec::new();
    let mut warnings = Vec::new();
    if value.policy_id != PLANNING_KNOWLEDGE_POLICY_ID
        || value.policy_version != PLANNING_KNOWLEDGE_POLICY_VERSION
        || value.needs.policy_id != PLANNING_KNOWLEDGE_POLICY_ID
        || value.needs.policy_version != PLANNING_KNOWLEDGE_POLICY_VERSION
    {
        stale_reasons.push("planning_policy".into());
    }
    if value.workspace_generation != current_generation {
        stale_reasons.push("workspace_generation".into());
    }
    let unavailable:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_owned_copies c JOIN knowledge_unit_heads h ON h.tenant_id=c.tenant_id AND h.workspace_id=c.workspace_id AND h.unit_id=c.unit_id WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.relation_name='planning_knowledge_manifests' AND c.row_id=$3 AND (c.redacted OR h.payload_erased OR NOT h.active))")
        .bind(tenant).bind(workspace).bind(value.id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if unavailable {
        stale_reasons.push("knowledge_unavailable".into());
    }
    let mut reviewed = std::collections::BTreeMap::new();
    for item in &value.selected {
        reviewed
            .entry((item.unit_id, item.unit_revision))
            .and_modify(|required| *required |= item.purposes.iter().copied().any(blocking))
            .or_insert_with(|| item.purposes.iter().copied().any(blocking));
    }
    for ((unit_id, unit_revision), required) in reviewed {
        let review = crate::knowledge_maintenance::current_unit_review_status(
            tx,
            tenant,
            workspace,
            principal,
            unit_id,
            unit_revision,
        )
        .await?;
        if review.needs_review {
            if required {
                stale_reasons.push("knowledge_needs_review".into());
            } else {
                warnings.push("reference_knowledge_needs_review".into());
            }
        }
        if let Some(valid_from) = review.valid_from {
            let not_effective: bool =
                sqlx::query_scalar("SELECT $1::timestamptz>pg_catalog.clock_timestamp()")
                    .bind(valid_from)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(storage_error)?;
            if not_effective {
                if required {
                    stale_reasons.push("knowledge_not_effective".into());
                } else {
                    warnings.push("reference_knowledge_not_effective".into());
                }
            }
        }
    }
    stale_reasons.sort();
    stale_reasons.dedup();
    warnings.sort();
    warnings.dedup();
    Ok((stale_reasons, warnings))
}

pub(crate) async fn require(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    stage: PlanningStage,
    owner: Uuid,
    guard: Option<&PlanningManifestGuard>,
) -> Result<Option<PlanningKnowledgeManifest>> {
    if let Some(g) = guard {
        g.validate()?;
    }
    crate::durable_knowledge::publisher_gate(tx).await?;
    let (_, ready, _) = crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    if !ready {
        let protected: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND active AND NOT payload_erased) OR EXISTS(SELECT 1 FROM planning_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND stage=$3 AND owner_id=$4 AND (payload_erased OR pg_catalog.jsonb_array_length(COALESCE(selected,'[]'::jsonb))>0 OR pg_catalog.jsonb_array_length(COALESCE(unresolved_needs,'[]'::jsonb))>0))")
            .bind(tenant).bind(workspace).bind(stage_name(stage)).bind(owner)
            .fetch_one(&mut **tx).await.map_err(storage_error)?;
        if protected {
            return Err(Error::KnowledgeUnavailable);
        }
    }
    let status = status(tx, tenant, workspace, principal, stage, owner).await?;
    let Some(manifest) = status.manifest else {
        let (program, scope): (Option<Uuid>, Option<Uuid>) = match stage {
            PlanningStage::Program => (Some(owner), None),
            PlanningStage::Scope => {
                let program=sqlx::query_scalar("SELECT program_id FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
                    .bind(tenant).bind(workspace).bind(owner).fetch_optional(&mut **tx).await.map_err(storage_error)?;
                (program, None)
            }
            PlanningStage::SliceCandidates => {
                let row:Option<(Uuid,Uuid)>=sqlx::query_as("SELECT cs.program_id,ns.id FROM native_scopes ns JOIN scope_candidate_sets cs ON cs.tenant_id=ns.tenant_id AND cs.workspace_id=ns.workspace_id AND cs.id=ns.source_candidate_set_id WHERE ns.tenant_id=$1 AND ns.workspace_id=$2 AND ns.id=$3")
                    .bind(tenant).bind(workspace).bind(owner).fetch_optional(&mut **tx).await.map_err(storage_error)?;
                row.map(|(p, s)| (Some(p), Some(s))).unwrap_or((None, None))
            }
        };
        let any_required:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_bindings b JOIN knowledge_unit_heads h ON h.tenant_id=b.tenant_id AND h.workspace_id=b.workspace_id AND h.unit_id=b.unit_id JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=CASE b.version_resolution WHEN 'pinned_revision' THEN b.pinned_revision ELSE h.accepted_revision END JOIN knowledge_publication_events e ON e.tenant_id=r.tenant_id AND e.workspace_id=r.workspace_id AND e.id=r.publication_event_id CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(e.event_payload->'planned'->'document'->'planning_briefs','[]'::jsonb)) brief WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.revision=h.accepted_revision AND b.active AND h.active AND NOT h.payload_erased AND NOT r.payload_erased AND b.purpose<>'reference' AND brief->>'stage'=$3 AND (b.binding_kind='workspace' OR (b.binding_kind='program' AND b.program_id=$4) OR (b.binding_kind='scope' AND b.scope_id=$5)))")
            .bind(tenant).bind(workspace).bind(stage_name(stage)).bind(program).bind(scope)
            .fetch_one(&mut **tx).await.map_err(storage_error)?;
        return if any_required {
            Err(Error::StaleContext)
        } else {
            Ok(None)
        };
    };
    if !status.stale_reasons.is_empty() || !manifest.unresolved_needs.is_empty() {
        return Err(Error::StaleContext);
    }
    let Some(guard) = guard else {
        return if manifest
            .selected
            .iter()
            .any(|v| v.purposes.iter().copied().any(blocking))
        {
            Err(Error::StaleContext)
        } else {
            Ok(Some(manifest))
        };
    };
    if guard.manifest_id != manifest.id
        || guard.digest != manifest.digest
        || guard.workspace_generation != manifest.workspace_generation
    {
        return Err(Error::StaleContext);
    }
    Ok(Some(manifest))
}
