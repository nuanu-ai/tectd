use super::status::load;
use super::*;

pub(crate) async fn capture(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    stage: PlanningStage,
    owner: Uuid,
    owner_revision: i64,
    input_revision: i64,
    request: Uuid,
    program: Option<Uuid>,
    scope: Option<Uuid>,
    supplied_task_context: Option<&PlanningTaskContext>,
    method: &PlanningMethodSnapshot,
) -> Result<PlanningKnowledgeManifest> {
    if let Some(task_context) = supplied_task_context {
        task_context.validate()?;
    }
    method.validate()?;
    if byte_digest(method.body.as_bytes()) != method.digest {
        return Err(Error::InvalidArguments);
    }
    if owner.is_nil() || owner_revision < 1 || input_revision < 0 || request.is_nil() {
        return Err(Error::InvalidArguments);
    }
    crate::durable_knowledge::publisher_gate(tx).await?;
    let (generation, ready, _) =
        crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    if !ready {
        let protected: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND active AND NOT payload_erased) \
             OR EXISTS(SELECT 1 FROM planning_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 \
               AND (payload_erased OR pg_catalog.jsonb_array_length(COALESCE(selected,'[]'::jsonb))>0 \
                 OR pg_catalog.jsonb_array_length(COALESCE(unresolved_needs,'[]'::jsonb))>0))",
        ).bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
        if protected {
            return Err(Error::KnowledgeUnavailable);
        }
    }
    sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended($1::text||':'||$2::text||':'||$3::text||':'||$4::text,0))")
        .bind(tenant).bind(workspace).bind(stage_name(stage)).bind(request)
        .execute(&mut **tx).await.map_err(storage_error)?;
    if let Some(row) = sqlx::query("SELECT owner_revision,input_revision,program_id,scope_id,task_context,payload_erased,needs,selected,unresolved_needs,digest,id,workspace_generation,task_context_digest,policy_id,policy_version FROM planning_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND stage=$3 AND owner_id=$4 AND request_id=$5")
        .bind(tenant).bind(workspace).bind(stage_name(stage)).bind(owner).bind(request)
        .fetch_optional(&mut **tx).await.map_err(storage_error)? {
        let stored_context: PlanningTaskContext = decode(row.try_get(4).map_err(storage_error)?)?;
        let exact = row.try_get::<Option<Uuid>,_>(2).map_err(storage_error)? == program
            && row.try_get::<Option<Uuid>,_>(3).map_err(storage_error)? == scope
            && supplied_task_context.is_none_or(|task_context| *task_context == stored_context);
        if !exact { return Err(Error::InputConflict); }
        if row.try_get::<bool,_>(5).map_err(storage_error)? { return Err(Error::KnowledgePayloadErased); }
        let id = row.try_get(10).map_err(storage_error)?;
        require_manifest_access(tx, tenant, workspace, principal, id).await?;
        return Ok(PlanningKnowledgeManifest {
            id,
            digest: row.try_get(9).map_err(storage_error)?, stage, owner_id: owner,
            owner_revision: row.try_get(0).map_err(storage_error)?, input_revision: row.try_get(1).map_err(storage_error)?, request_id: request,
            policy_id: row.try_get(13).map_err(storage_error)?, policy_version: row.try_get(14).map_err(storage_error)?,
            task_context_digest: row.try_get(12).map_err(storage_error)?, task_context: stored_context,
            workspace_generation: row.try_get(11).map_err(storage_error)?,
            needs: decode(row.try_get(6).map_err(storage_error)?)?,
            selected: decode(row.try_get(7).map_err(storage_error)?)?,
            unresolved_needs: decode(row.try_get(8).map_err(storage_error)?)?,
        });
    }

    let task_context = match supplied_task_context {
        Some(value) => value.clone(),
        None => load(tx, tenant, workspace, principal, stage, owner)
            .await?
            .map(|manifest| manifest.task_context)
            .unwrap_or_default(),
    };
    let owner_access: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let rows = sqlx::query(
        "SELECT b.id,h.unit_id,CASE b.version_resolution WHEN 'pinned_revision' THEN b.pinned_revision ELSE h.accepted_revision END,b.purpose,b.binding_kind,h.active,h.payload_erased,h.lifecycle,h.access_scope,r.access_scope,r.payload_erased,r.publication_event_id,r.rdf_digest,e.event_payload,r.contract_version \
         FROM knowledge_bindings b JOIN knowledge_unit_heads h ON h.tenant_id=b.tenant_id AND h.workspace_id=b.workspace_id AND h.unit_id=b.unit_id \
         LEFT JOIN knowledge_revisions r ON r.tenant_id=b.tenant_id AND r.workspace_id=b.workspace_id AND r.unit_id=b.unit_id AND r.revision=CASE b.version_resolution WHEN 'pinned_revision' THEN b.pinned_revision ELSE h.accepted_revision END \
         LEFT JOIN knowledge_publication_events e ON e.tenant_id=r.tenant_id AND e.workspace_id=r.workspace_id AND e.id=r.publication_event_id \
         WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.revision=h.accepted_revision AND b.active \
           AND (b.binding_kind='workspace' OR (b.binding_kind='program' AND b.program_id=$3) OR (b.binding_kind='scope' AND b.scope_id=$4)) ORDER BY h.unit_id,b.id")
        .bind(tenant).bind(workspace).bind(program).bind(scope)
        .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut selected = Vec::new();
    let mut unresolved = Vec::new();
    for row in rows {
        let unit: Uuid = row.try_get(1).map_err(storage_error)?;
        let revision: i64 = row.try_get(2).map_err(storage_error)?;
        let purpose: KnowledgeBindingPurpose = decode(serde_json::Value::String(
            row.try_get(3).map_err(storage_error)?,
        ))?;
        let inaccessible = (row.try_get::<String, _>(8).map_err(storage_error)? == "owners_only"
            || row
                .try_get::<Option<String>, _>(9)
                .map_err(storage_error)?
                .as_deref()
                == Some("owners_only"))
            && !owner_access;
        let available = row.try_get::<bool, _>(5).map_err(storage_error)?
            && !row.try_get::<bool, _>(6).map_err(storage_error)?
            && row.try_get::<String, _>(7).map_err(storage_error)? == "active"
            && row.try_get::<Option<bool>, _>(10).map_err(storage_error)? == Some(false)
            && row
                .try_get::<Option<Uuid>, _>(11)
                .map_err(storage_error)?
                .is_some();
        if inaccessible || !available {
            let has_stage_brief = row
                .try_get::<Option<serde_json::Value>, _>(13)
                .map_err(storage_error)?
                .and_then(|v| v.pointer("/planned/document/planning_briefs").cloned())
                .and_then(|v| v.as_array().cloned())
                .is_some_and(|values| {
                    values
                        .iter()
                        .any(|v| v.get("stage").and_then(|x| x.as_str()) == Some(stage_name(stage)))
                });
            if blocking(purpose) && has_stage_brief {
                unresolved.push(PlanningKnowledgeGap {
                    kind: PlanningKnowledgeGapKind::RequiredUnavailable,
                    unit_id: if inaccessible { Uuid::nil() } else { unit },
                    unit_revision: revision,
                    brief_local_id: None,
                    reason: if inaccessible {
                        "resource_inaccessible"
                    } else {
                        "resource_unavailable"
                    }
                    .into(),
                });
            }
            continue;
        }
        let event: Uuid = row
            .try_get::<Option<Uuid>, _>(11)
            .map_err(storage_error)?
            .ok_or(Error::InternalInvariant)?;
        match row
            .try_get::<Option<String>, _>(14)
            .map_err(storage_error)?
            .as_deref()
        {
            Some("dk-1") => {
                let legacy = crate::durable_knowledge::context::load_revision(
                    tx,
                    tenant,
                    workspace,
                    unit,
                    Some(revision),
                    true,
                )
                .await?
                .ok_or(Error::InternalInvariant)?;
                if legacy.publication_event_id != event
                    || row
                        .try_get::<Option<String>, _>(12)
                        .map_err(storage_error)?
                        .as_deref()
                        != Some(legacy.rdf_digest.as_str())
                {
                    return Err(Error::InternalInvariant);
                }
                continue;
            }
            Some("dk-2") => {}
            _ => return Err(Error::InternalInvariant),
        }
        let verified = crate::knowledge_lifecycle::verify_publication_event(
            tx, tenant, workspace, unit, revision, event, true,
        )
        .await?;
        if row
            .try_get::<Option<String>, _>(12)
            .map_err(storage_error)?
            .as_deref()
            != Some(verified.rdf_digest.as_str())
        {
            return Err(Error::InternalInvariant);
        }
        let Some(document) = verified.input.planned.document.as_ref() else {
            continue;
        };
        if document.planning_briefs.is_empty() {
            continue;
        }
        let expected = rdf::build(&verified.input)?;
        let native = rdf::native_rows(tx, tenant, workspace, unit, revision, event, true).await?;
        rdf::validate_rows(&native, &expected)?;
        let review = crate::knowledge_maintenance::current_unit_review_status(
            tx, tenant, workspace, principal, unit, revision,
        )
        .await?;
        let needs_review = review.needs_review;
        let not_effective = if let Some(valid_from) = review.valid_from.as_deref() {
            sqlx::query_scalar("SELECT $1::timestamptz>pg_catalog.clock_timestamp()")
                .bind(valid_from)
                .fetch_one(&mut **tx)
                .await
                .map_err(storage_error)?
        } else {
            false
        };
        for brief in document.planning_briefs.iter().filter(|b| b.stage == stage) {
            match applicable(brief, &task_context) {
                Ok(false) => continue,
                Err(()) if blocking(purpose) => {
                    unresolved.push(PlanningKnowledgeGap {
                        kind: PlanningKnowledgeGapKind::NeedsContext,
                        unit_id: unit,
                        unit_revision: revision,
                        brief_local_id: Some(brief.local_id.clone()),
                        reason: "required_selector_context_missing".into(),
                    });
                    continue;
                }
                Err(()) => continue,
                Ok(true) => {}
            }
            if not_effective {
                if blocking(purpose) {
                    unresolved.push(PlanningKnowledgeGap {
                        kind: PlanningKnowledgeGapKind::RequiredUnavailable,
                        unit_id: unit,
                        unit_revision: revision,
                        brief_local_id: Some(brief.local_id.clone()),
                        reason: "knowledge_not_effective".into(),
                    });
                }
                continue;
            }
            if needs_review && blocking(purpose) {
                unresolved.push(PlanningKnowledgeGap {
                    kind: PlanningKnowledgeGapKind::NeedsReview,
                    unit_id: unit,
                    unit_revision: revision,
                    brief_local_id: Some(brief.local_id.clone()),
                    reason: "knowledge_needs_review".into(),
                });
            }
            selected.push(PlanningKnowledgeItem {
                unit_id: unit,
                unit_revision: revision,
                brief_local_id: brief.local_id.clone(),
                rdf_digest: verified.rdf_digest.clone(),
                purposes: vec![purpose],
                why_included: vec![format!(
                    "{}_binding:{}{}",
                    row.try_get::<String, _>(4).map_err(storage_error)?,
                    purpose_name(purpose),
                    if needs_review { ":needs_review" } else { "" }
                )],
                instruction: brief.instruction.clone(),
                conditions: brief.conditions.clone(),
                exceptions: brief.exceptions.clone(),
                declared_purpose: brief.purpose.clone(),
                selectors: brief.selectors.clone(),
            });
        }
    }
    selected.sort_by(|a, b| {
        (a.unit_id, a.unit_revision, &a.brief_local_id).cmp(&(
            b.unit_id,
            b.unit_revision,
            &b.brief_local_id,
        ))
    });
    let mut merged: Vec<PlanningKnowledgeItem> = Vec::new();
    for mut item in selected {
        if let Some(previous) = merged.last_mut().filter(|previous| {
            (
                previous.unit_id,
                previous.unit_revision,
                &previous.brief_local_id,
            ) == (item.unit_id, item.unit_revision, &item.brief_local_id)
        }) {
            previous.purposes.append(&mut item.purposes);
            previous.why_included.append(&mut item.why_included);
            previous
                .purposes
                .sort_by_key(|purpose| purpose_rank(*purpose));
            previous.purposes.dedup();
            previous.why_included.sort();
            previous.why_included.dedup();
        } else {
            merged.push(item);
        }
    }
    let selected = merged;
    unresolved.sort_by(|a, b| {
        (a.unit_id, a.unit_revision, &a.brief_local_id, &a.reason).cmp(&(
            b.unit_id,
            b.unit_revision,
            &b.brief_local_id,
            &b.reason,
        ))
    });
    unresolved.dedup_by(|a, b| a == b);
    let id = Uuid::new_v4();
    let needs = planning_knowledge_need(stage, method.clone());
    let task_context_digest = digest(&task_context)?;
    let manifest_digest = digest(&(
        id,
        stage,
        owner,
        owner_revision,
        input_revision,
        request,
        PLANNING_KNOWLEDGE_POLICY_ID,
        PLANNING_KNOWLEDGE_POLICY_VERSION,
        &task_context_digest,
        generation,
        &needs,
        &selected,
        &unresolved,
    ))?;
    let manifest = PlanningKnowledgeManifest {
        id,
        digest: manifest_digest,
        stage,
        owner_id: owner,
        owner_revision,
        input_revision,
        request_id: request,
        policy_id: PLANNING_KNOWLEDGE_POLICY_ID.into(),
        policy_version: PLANNING_KNOWLEDGE_POLICY_VERSION.into(),
        task_context_digest,
        task_context: task_context.clone(),
        workspace_generation: generation,
        needs,
        selected,
        unresolved_needs: unresolved,
    };
    sqlx::query("INSERT INTO planning_knowledge_manifests(id,tenant_id,workspace_id,stage,owner_id,owner_revision,input_revision,request_id,program_id,scope_id,policy_id,policy_version,task_context_digest,task_context,workspace_generation,needs,selected,unresolved_needs,digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)")
        .bind(id).bind(tenant).bind(workspace).bind(stage_name(stage)).bind(owner).bind(owner_revision).bind(input_revision).bind(request).bind(program).bind(scope)
        .bind(&manifest.policy_id).bind(&manifest.policy_version).bind(&manifest.task_context_digest).bind(json(&task_context)?).bind(generation)
        .bind(json(&manifest.needs)?).bind(json(&manifest.selected)?).bind(json(&manifest.unresolved_needs)?).bind(&manifest.digest)
        .execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_maintenance::retire_planning_owner_consumers(
        tx,
        tenant,
        workspace,
        stage_name(stage),
        owner,
        id,
    )
    .await?;
    super::lineage::register_manifest_lineage(tx, tenant, workspace, id).await?;
    Ok(manifest)
}
