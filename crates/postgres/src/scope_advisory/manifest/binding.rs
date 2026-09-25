fn authored_graph_binding(
    manifest: &ScopeConstructorManifest,
    non_goal: &[String],
) -> Result<(Vec<AntiBloatObligationLink>, String, String)> {
    authored_graph_binding_for(
        manifest,
        &manifest.baseline_id,
        manifest.source.candidate_set_revision,
        non_goal,
    )
}

pub(crate) fn authored_graph_binding_for(
    manifest: &ScopeConstructorManifest,
    selected_id: &ScopeAlternativeId,
    selected_revision: i64,
    non_goal: &[String],
) -> Result<(Vec<AntiBloatObligationLink>, String, String)> {
    if !is_source_authored_identity(&manifest.constructor)
        || (manifest.constructor == legacy_source_authored_identity()
            && (manifest.emitted.iter().any(|alternative| {
                alternative
                    .material
                    .candidates
                    .iter()
                    .any(|candidate| !candidate.grounding.is_source_grounded())
            }) || manifest.rejected.iter().any(|rejected| {
                rejected
                    .alternative
                    .material
                    .candidates
                    .iter()
                    .any(|candidate| !candidate.grounding.is_source_grounded())
            })))
    {
        return Err(Error::InvalidSource);
    }
    let selected = manifest.eligible(selected_id).ok_or(Error::InvalidSource)?;
    if selected_revision < manifest.source.candidate_set_revision {
        return Err(Error::StaleRevision);
    }
    let mut obligations = std::collections::BTreeSet::new();
    for obligation in &manifest.obligations {
        if obligation.id != obligation.source_input_id
            || Uuid::parse_str(&obligation.id).is_err()
            || !obligations.insert(obligation.id.clone())
        {
            return Err(Error::InvalidSource);
        }
    }
    let non_goal_set = non_goal.iter().collect::<std::collections::BTreeSet<_>>();
    if non_goal_set.len() != non_goal.len()
        || non_goal.windows(2).any(|pair| pair[0] >= pair[1])
        || non_goal_set
            .iter()
            .any(|id| !obligations.contains(id.as_str()))
    {
        return Err(Error::InvalidSource);
    }
    let mut links = Vec::new();
    let mut covered = std::collections::BTreeSet::new();
    for goal in &selected.material.goals {
        let source_id = goal.source_ref_id.to_string();
        if !obligations.contains(&source_id)
            || non_goal_set.contains(&source_id)
            || !covered.insert((source_id.clone(), goal.id))
        {
            return Err(Error::InvalidSource);
        }
        links.push(AntiBloatObligationLink {
            obligation_id: source_id,
            goal_id: goal.id,
        });
    }
    let mut accounted = covered
        .iter()
        .map(|(id, _)| id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    accounted.extend(non_goal.iter().map(String::as_str));
    if accounted != obligations.iter().map(String::as_str).collect() {
        return Err(Error::InvalidSource);
    }
    links.sort_by(|a, b| (&a.obligation_id, a.goal_id).cmp(&(&b.obligation_id, b.goal_id)));

    let mut dependencies = selected
        .material
        .candidates
        .iter()
        .map(|candidate| {
            let mut ids = candidate.dependencies.clone();
            ids.sort();
            (candidate.id, ids)
        })
        .collect::<Vec<_>>();
    dependencies.sort_by_key(|(id, _)| *id);
    let digest_payload = serde_json::to_vec(&(
        "tect.anti-bloat-dependency-graph/1",
        &manifest.source.digest,
        &manifest.whole_set_digest,
        &selected.material_digest,
        selected_revision,
        &dependencies,
    ))
    .map_err(storage_error)?;
    let dependency_digest = format!("{:x}", sha2::Sha256::digest(digest_payload));
    let provenance = format!(
        "tect.source-authored-graph-binding/1:source={}:whole={}:material={}:revision={}:dependencies={}",
        manifest.source.digest,
        manifest.whole_set_digest,
        selected.material_digest,
        selected_revision,
        dependency_digest,
    );
    Ok((links, dependency_digest, provenance))
}

async fn insert_selected_graph_binding(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: &ScopeConstructorManifest,
    opportunity_id: Uuid,
    selected_id: &ScopeAlternativeId,
    selected_revision: i64,
    caller_link_id: Uuid,
    caller_request_id: Uuid,
) -> Result<()> {
    let selected = manifest.eligible(selected_id).ok_or(Error::InvalidSource)?;
    let non_goal = trusted_non_goal_source_obligation_ids(
        tx,
        tenant,
        workspace,
        manifest,
        selected.material.boundary,
    )
    .await?;
    let (links, dependency_digest, provenance) =
        authored_graph_binding_for(manifest, selected_id, selected_revision, &non_goal)?;
    let provenance = format!(
        "{provenance}:selected={}:caller={caller_link_id}:receipt={caller_request_id}",
        selected_id.0
    );
    let saved_payload: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND set_revision=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest.source.candidate_set_id)
    .bind(selected_revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .flatten();
    let saved: ResolvedCandidateDraft =
        serde_json::from_value(saved_payload.ok_or(Error::InputConflict)?)
            .map_err(storage_error)?;
    if saved != selected.material
        || scope_candidate_material_digest(&Sha256ScopeDigest, &saved)? != selected.material_digest
    {
        return Err(Error::InputConflict);
    }
    sqlx::query(
        "INSERT INTO scope_anti_bloat_bindings \
         (tenant_id,workspace_id,candidate_set_id,opportunity_id,candidate_set_revision, \
          source_digest,dependency_digest,obligation_links,non_goal_source_obligation_ids,mandatory_policy_obligation_ids,provenance, \
          selected_draft_revision,selected_material_digest,selected_alternative_id, \
          selected_caller_link_id,selected_caller_request_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest.source.candidate_set_id)
    .bind(opportunity_id)
    .bind(selected_revision)
    .bind(&manifest.source.digest)
    .bind(dependency_digest)
    .bind(serde_json::to_value(links).map_err(storage_error)?)
    .bind(serde_json::to_value(non_goal).map_err(storage_error)?)
    .bind(serde_json::json!([]))
    .bind(provenance)
    .bind(selected_revision)
    .bind(&selected.material_digest)
    .bind(&selected_id.0)
    .bind(caller_link_id)
    .bind(caller_request_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn insert_authored_graph_binding(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &ScopeManifestRecord,
) -> Result<()> {
    let non_goal = trusted_non_goal_source_obligation_ids(
        tx,
        tenant,
        workspace,
        &record.manifest,
        record
            .manifest
            .eligible(&record.manifest.baseline_id)
            .ok_or(Error::InvalidSource)?
            .material
            .boundary,
    )
    .await?;
    let (links, dependency_digest, provenance) =
        authored_graph_binding(&record.manifest, &non_goal)?;
    sqlx::query(
        "INSERT INTO scope_anti_bloat_bindings \
         (tenant_id,workspace_id,candidate_set_id,opportunity_id,candidate_set_revision, \
          source_digest,dependency_digest,obligation_links,non_goal_source_obligation_ids,mandatory_policy_obligation_ids,provenance) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.candidate_set_id)
    .bind(record.opportunity_id)
    .bind(record.manifest.source.candidate_set_revision)
    .bind(&record.manifest.source.digest)
    .bind(dependency_digest)
    .bind(serde_json::to_value(links).map_err(storage_error)?)
    .bind(serde_json::to_value(non_goal).map_err(storage_error)?)
    .bind(serde_json::json!([]))
    .bind(provenance)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn require_authored_graph_binding(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &ScopeManifestRecord,
) -> Result<()> {
    let non_goal = trusted_non_goal_source_obligation_ids(
        tx,
        tenant,
        workspace,
        &record.manifest,
        record
            .manifest
            .eligible(&record.manifest.baseline_id)
            .ok_or(Error::InvalidSource)?
            .material
            .boundary,
    )
    .await?;
    let (links, dependency_digest, provenance) =
        authored_graph_binding(&record.manifest, &non_goal)?;
    let actual: Option<(serde_json::Value, serde_json::Value, serde_json::Value, String, String)> = sqlx::query_as(
        "SELECT obligation_links,non_goal_source_obligation_ids,mandatory_policy_obligation_ids,dependency_digest,provenance \
         FROM scope_anti_bloat_bindings WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND candidate_set_revision=$4 AND opportunity_id=$5 \
         AND source_digest=$6",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.candidate_set_id)
    .bind(record.manifest.source.candidate_set_revision)
    .bind(record.opportunity_id)
    .bind(&record.manifest.source.digest)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if actual
        != Some((
            serde_json::to_value(links).map_err(storage_error)?,
            serde_json::to_value(non_goal).map_err(storage_error)?,
            serde_json::json!([]),
            dependency_digest,
            provenance,
        ))
    {
        return Err(Error::StorageUnavailable);
    }
    Ok(())
}
