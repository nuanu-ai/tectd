use super::*;

pub(crate) async fn scope_open_replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &OpenScope,
) -> Result<Option<OpenScopeOutcome>> {
    let row:Option<(serde_json::Value,Option<serde_json::Value>)>=sqlx::query_as(
        "SELECT origin_payload,origin_result FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND origin_request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    match row {
        None => Ok(None),
        Some((payload, result)) => {
            if payload != json(request)? {
                return Err(Error::InputConflict);
            }
            let prior: OpenScopeOutcome = decode(result.ok_or(Error::InternalInvariant)?)?;
            let context = match prior {
                OpenScopeOutcome::Created(value) | OpenScopeOutcome::Replay(value) => value,
            };
            Ok(Some(OpenScopeOutcome::Replay(context)))
        }
    }
}

#[allow(clippy::type_complexity)]
async fn source_basis(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &OpenScope,
    lock: bool,
) -> Result<(ScopeOpenBasis, CandidateEntity)> {
    let sql = if lock {
        "SELECT s.revision,s.status,s.current_snapshot_id,s.input_cursor,s.latest_input,d.payload,r.payload \
         FROM scope_candidate_sets s \
         JOIN programs p ON p.tenant_id=s.tenant_id AND p.workspace_id=s.workspace_id AND p.id=s.program_id \
         JOIN LATERAL (SELECT payload FROM scope_candidate_drafts WHERE tenant_id=s.tenant_id AND workspace_id=s.workspace_id AND candidate_set_id=s.id ORDER BY set_revision DESC LIMIT 1) d ON true \
         JOIN LATERAL (SELECT payload FROM scope_candidate_reviews WHERE tenant_id=s.tenant_id AND workspace_id=s.workspace_id AND candidate_set_id=s.id ORDER BY set_revision DESC LIMIT 1) r ON true \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3 FOR UPDATE OF s,p"
    } else {
        "SELECT s.revision,s.status,s.current_snapshot_id,s.input_cursor,s.latest_input,d.payload,r.payload \
         FROM scope_candidate_sets s \
         JOIN programs p ON p.tenant_id=s.tenant_id AND p.workspace_id=s.workspace_id AND p.id=s.program_id \
         JOIN LATERAL (SELECT payload FROM scope_candidate_drafts WHERE tenant_id=s.tenant_id AND workspace_id=s.workspace_id AND candidate_set_id=s.id ORDER BY set_revision DESC LIMIT 1) d ON true \
         JOIN LATERAL (SELECT payload FROM scope_candidate_reviews WHERE tenant_id=s.tenant_id AND workspace_id=s.workspace_id AND candidate_set_id=s.id ORDER BY set_revision DESC LIMIT 1) r ON true \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3"
    };
    let row: Option<(
        i64,
        String,
        Option<Uuid>,
        i64,
        i64,
        serde_json::Value,
        serde_json::Value,
    )> = sqlx::query_as(sql)
        .bind(tenant)
        .bind(workspace)
        .bind(request.candidate_set_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
    let (revision, status, snapshot, input_cursor, latest_input, draft_value, review_value) =
        row.ok_or(Error::NotFound)?;
    if revision != request.candidate_set_revision {
        return Err(Error::StaleRevision);
    }
    if snapshot != Some(request.candidate_snapshot_id) || input_cursor != latest_input {
        return Err(Error::StaleContext);
    }
    if status != "ready" {
        return Err(Error::Forbidden);
    }
    let draft: ResolvedCandidateDraft = decode(draft_value)?;
    let review: ScopeCandidateReview = decode(review_value)?;
    if review.verdict != ReviewVerdict::Ready {
        return Err(Error::Forbidden);
    }
    let candidate = draft
        .candidates
        .into_iter()
        .find(|c| c.id == request.candidate_id)
        .ok_or(Error::NotFound)?;
    if candidate.revision != request.candidate_revision {
        return Err(Error::StaleRevision);
    }
    if !review
        .candidate_decisions
        .iter()
        .any(|d| d.candidate_id == candidate.id && d.decision == CandidateDecisionKind::Accept)
    {
        return Err(Error::Forbidden);
    }
    let basis = ScopeOpenBasis {
        boundary: draft.boundary,
        title: candidate.title.clone(),
        outcome: candidate.outcome.clone(),
        includes: candidate.includes.clone(),
        excludes: candidate.excludes.clone(),
        source_candidate_set_revision: revision,
        source_snapshot_id: request.candidate_snapshot_id,
    };
    Ok((basis, candidate))
}

pub(crate) async fn scope_open_basis(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &OpenScope,
) -> Result<ScopeOpenBasis> {
    Ok(source_basis(tx, tenant, workspace, request, true).await?.0)
}

pub(crate) async fn open_scope(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    _session: Uuid,
    request: &OpenScope,
    material: &SlicePlanningSnapshotMaterial,
) -> Result<OpenScopeOutcome> {
    if let Some(value) = scope_open_replay(tx, tenant, workspace, request).await? {
        return Ok(value);
    }
    let (basis, _) = source_basis(tx, tenant, workspace, request, true).await?;
    if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND source_candidate_id=$3)")
        .bind(tenant).bind(workspace).bind(request.candidate_id).fetch_one(&mut **tx).await.map_err(storage_error)? {return Err(Error::Forbidden)}
    let scope_id = Uuid::new_v4();
    let set_id = Uuid::new_v4();
    let payload = json(request)?;
    sqlx::query("INSERT INTO native_scopes(id,tenant_id,workspace_id,source_candidate_set_id,source_candidate_set_revision,source_snapshot_id,source_candidate_id,source_candidate_revision,boundary,title,outcome,includes,excludes,origin_request_id,origin_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)")
        .bind(scope_id).bind(tenant).bind(workspace).bind(request.candidate_set_id).bind(request.candidate_set_revision).bind(request.candidate_snapshot_id).bind(request.candidate_id).bind(request.candidate_revision)
        .bind(json(&basis.boundary)?.as_str().unwrap()).bind(&basis.title).bind(&basis.outcome).bind(json(&basis.includes)?).bind(json(&basis.excludes)?).bind(request.request_id).bind(&payload)
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO slice_candidate_sets(id,tenant_id,workspace_id,scope_id) VALUES($1,$2,$3,$4)",
    )
    .bind(set_id)
    .bind(tenant)
    .bind(workspace)
    .bind(scope_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    insert_snapshot(
        tx,
        tenant,
        workspace,
        set_id,
        scope_id,
        1,
        0,
        request.candidate_set_revision,
        request.candidate_snapshot_id,
        material,
        &[],
    )
    .await?;
    sqlx::query("UPDATE native_scopes SET slice_candidate_set_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(scope_id).bind(set_id).execute(&mut **tx).await.map_err(storage_error)?;
    let context = load_context(tx, tenant, workspace, scope_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let outcome = OpenScopeOutcome::Created(OpenScopeContext {
        scope: context.scope.clone(),
        planning: context,
    });
    sqlx::query("UPDATE native_scopes SET origin_result=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(scope_id).bind(json(&outcome)?).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_snapshot(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    set_id: Uuid,
    scope_id: Uuid,
    sequence: i64,
    latest_input: i64,
    source_set_revision: i64,
    source_snapshot_id: Uuid,
    material: &SlicePlanningSnapshotMaterial,
    result_ids: &[Uuid],
) -> Result<Uuid> {
    let id = Uuid::new_v4();
    let scope_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query("INSERT INTO slice_planning_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,scope_revision,source_candidate_set_revision,source_snapshot_id,planning_latest_input,method,registry_revision,registry_digest,rules,catalogue,result_ids) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)")
        .bind(id).bind(tenant).bind(workspace).bind(set_id).bind(sequence).bind(scope_revision).bind(source_set_revision).bind(source_snapshot_id).bind(latest_input)
        .bind(json(&material.method)?).bind(&material.registry_revision).bind(&material.registry_digest).bind(json(&material.rules)?).bind(json(&material.catalogue)?).bind(result_ids)
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_candidate_sets SET current_snapshot_id=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(set_id).bind(id).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(id)
}
