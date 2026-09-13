use super::*;

pub(crate) async fn save_draft(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &SaveSliceCandidateDraft,
) -> Result<SliceCandidateContext> {
    let payload = json(request)?;
    if let Some(v) = receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "save_slice_draft",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(v);
    }
    let locked = lock_set(
        tx,
        tenant,
        workspace,
        request.scope_id,
        request.candidate_set_id,
    )
    .await?;
    if let Some(v) = receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "save_slice_draft",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(v);
    }
    guard(
        &locked,
        request.revision,
        request.snapshot_id,
        request.input_cursor,
    )?;
    if locked.1 == "ready" || locked.1 == "blocked" {
        return Err(Error::Forbidden);
    }
    let previous = load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::NotFound)?
        .draft;
    let resolved = resolve_draft(
        tx,
        tenant,
        workspace,
        request.scope_id,
        &request.draft,
        previous.as_ref(),
    )
    .await?;
    let next = locked.0.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query("INSERT INTO slice_candidate_drafts(tenant_id,workspace_id,candidate_set_id,set_revision,payload) VALUES($1,$2,$3,$4,$5)")
        .bind(tenant).bind(workspace).bind(request.candidate_set_id).bind(next).bind(json(&resolved)?).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_candidate_sets SET revision=$4,status='review_required',input_cursor=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.candidate_set_id).bind(next).bind(request.input_cursor).execute(&mut **tx).await.map_err(storage_error)?;
    let result = load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    save_receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "save_slice_draft",
        request.request_id,
        payload,
        &result,
    )
    .await?;
    Ok(result)
}

async fn resolve_draft(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
    draft: &SliceCandidateDraft,
    previous: Option<&ResolvedSliceCandidateDraft>,
) -> Result<ResolvedSliceCandidateDraft> {
    let previous_nodes = previous
        .map(|p| {
            p.nodes
                .iter()
                .map(|n| (n.id(), n.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let opened: BTreeSet<Uuid>=sqlx::query_scalar("SELECT candidate_id FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3")
        .bind(tenant).bind(workspace).bind(scope).fetch_all(&mut **tx).await.map_err(storage_error)?.into_iter().collect();
    let valid_results: BTreeSet<Uuid> = sqlx::query_scalar(
        "SELECT id FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?
    .into_iter()
    .collect();
    let mut local_ids = BTreeMap::new();
    let mut node_ids = BTreeSet::new();
    for node in &draft.nodes {
        let identity = match node {
            SliceCandidateDraftNode::Work { identity, .. }
            | SliceCandidateDraftNode::Decision { identity, .. } => identity,
        };
        let id = if let Some(local) = &identity.local {
            let id = Uuid::new_v4();
            local_ids.insert(local.clone(), id);
            id
        } else {
            let id = identity.candidate_id.ok_or(Error::InvalidArguments)?;
            let old = previous_nodes.get(&id).ok_or(Error::InvalidArguments)?;
            if old.revision() != identity.revision.ok_or(Error::InvalidArguments)? {
                return Err(Error::StaleRevision);
            }
            id
        };
        if !node_ids.insert(id) {
            return Err(Error::InvalidArguments);
        }
    }
    let resolve_ref = |r: &SliceCandidateRef| -> Result<Uuid> {
        match r {
            SliceCandidateRef::Local { local } => {
                local_ids.get(local).copied().ok_or(Error::InvalidArguments)
            }
            SliceCandidateRef::Existing {
                candidate_id,
                revision,
            } => {
                let old = previous_nodes
                    .get(candidate_id)
                    .ok_or(Error::InvalidArguments)?;
                if old.revision() != *revision {
                    return Err(Error::StaleRevision);
                }
                Ok(*candidate_id)
            }
        }
    };
    let mut nodes = Vec::new();
    for node in &draft.nodes {
        let (identity, change) = match node {
            SliceCandidateDraftNode::Work {
                identity,
                change_rationale,
                ..
            }
            | SliceCandidateDraftNode::Decision {
                identity,
                change_rationale,
                ..
            } => (identity, change_rationale),
        };
        let id = identity
            .candidate_id
            .unwrap_or_else(|| local_ids[identity.local.as_ref().unwrap()]);
        let base_revision = identity.revision.unwrap_or(1);
        let candidate = match node {
            SliceCandidateDraftNode::Work {
                title,
                outcome,
                includes,
                excludes,
                dependencies,
                proof,
                pipeline,
                pipeline_reason,
                why_lightweight_insufficient,
                why_further_vertical_split_not_viable,
                source_result_ids,
                ..
            } => {
                if source_result_ids
                    .iter()
                    .any(|id| !valid_results.contains(id))
                {
                    return Err(Error::InvalidArguments);
                }
                SliceCandidateNode::Work {
                    id,
                    revision: base_revision,
                    title: title.clone(),
                    outcome: outcome.clone(),
                    includes: includes.clone(),
                    excludes: excludes.clone(),
                    dependencies: dependencies
                        .iter()
                        .map(&resolve_ref)
                        .collect::<Result<Vec<_>>>()?,
                    proof: proof.clone(),
                    pipeline: *pipeline,
                    pipeline_reason: pipeline_reason.clone(),
                    why_lightweight_insufficient: why_lightweight_insufficient.clone(),
                    why_further_vertical_split_not_viable: why_further_vertical_split_not_viable
                        .clone(),
                    source_result_ids: source_result_ids.clone(),
                }
            }
            SliceCandidateDraftNode::Decision {
                title,
                question,
                resolution_criteria,
                dependencies,
                source_result_ids,
                ..
            } => {
                if source_result_ids
                    .iter()
                    .any(|id| !valid_results.contains(id))
                {
                    return Err(Error::InvalidArguments);
                }
                SliceCandidateNode::Decision {
                    id,
                    revision: base_revision,
                    title: title.clone(),
                    question: question.clone(),
                    resolution_criteria: resolution_criteria.clone(),
                    dependencies: dependencies
                        .iter()
                        .map(&resolve_ref)
                        .collect::<Result<Vec<_>>>()?,
                    source_result_ids: source_result_ids.clone(),
                }
            }
        };
        let candidate = if let Some(old) = previous_nodes.get(&id) {
            if &candidate == old {
                candidate
            } else {
                if opened.contains(&id) || change.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(Error::Forbidden);
                }
                with_revision(
                    candidate,
                    old.revision()
                        .checked_add(1)
                        .ok_or(Error::StorageUnavailable)?,
                )
            }
        } else {
            candidate
        };
        nodes.push(candidate);
    }
    for id in &opened {
        if !nodes.iter().any(|n| n.id() == *id) {
            return Err(Error::Forbidden);
        }
    }
    let mut supersessions = Vec::new();
    for item in &draft.supersessions {
        let old = previous_nodes
            .get(&item.candidate_id)
            .ok_or(Error::InvalidArguments)?;
        if old.revision() != item.revision {
            return Err(Error::StaleRevision);
        }
        if opened.contains(&item.candidate_id)
            || nodes.iter().any(|n| n.id() == item.candidate_id)
            || item.reason.trim().is_empty()
        {
            return Err(Error::Forbidden);
        }
        if item
            .source_result_ids
            .iter()
            .any(|id| !valid_results.contains(id))
        {
            return Err(Error::InvalidArguments);
        }
        let replacements = item
            .replacements
            .iter()
            .map(&resolve_ref)
            .collect::<Result<Vec<_>>>()?;
        if replacements.is_empty() && item.source_result_ids.is_empty() {
            return Err(Error::InvalidArguments);
        }
        if replacements
            .iter()
            .any(|id| !nodes.iter().any(|n| n.id() == *id))
        {
            return Err(Error::InvalidArguments);
        }
        supersessions.push(ResolvedSliceCandidateSupersession {
            candidate_id: item.candidate_id,
            revision: item.revision,
            reason: item.reason.clone(),
            source_result_ids: item.source_result_ids.clone(),
            replacement_candidate_ids: replacements,
        });
    }
    for old in previous_nodes.keys() {
        if !nodes.iter().any(|n| n.id() == *old)
            && !supersessions.iter().any(|s| s.candidate_id == *old)
        {
            return Err(Error::InvalidArguments);
        }
    }
    validate_slice_graph(&nodes)?;
    Ok(ResolvedSliceCandidateDraft {
        coverage_summary: draft.coverage_summary.clone(),
        nodes,
        supersessions,
    })
}
fn with_revision(mut node: SliceCandidateNode, revision: i64) -> SliceCandidateNode {
    match &mut node {
        SliceCandidateNode::Work { revision: r, .. }
        | SliceCandidateNode::Decision { revision: r, .. } => *r = revision,
    };
    node
}
