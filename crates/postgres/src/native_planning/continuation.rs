use super::*;

pub(crate) async fn save_review(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &ReviewSliceCandidateSet,
) -> Result<SliceCandidateContext> {
    let payload = json(request)?;
    if let Some(v) = receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "review_slice_set",
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
    guard(
        &locked,
        request.revision,
        request.snapshot_id,
        request.input_cursor,
    )?;
    require_review_status(&locked.1)?;
    if request.review.verdict == SlicePlanReviewVerdict::Ready
        && request.review.findings.iter().any(|f| f.material)
    {
        return Err(Error::InvalidArguments);
    }
    let ctx = load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::NotFound)?;
    let draft = ctx.draft.as_ref().ok_or(Error::InvalidArguments)?;
    for node in &draft.nodes {
        if let SliceCandidateNode::Work {
            source_checkpoint: Some(source),
            ..
        } = node
        {
            crate::pipeline_execution::validate_candidate_lineage(
                tx,
                tenant,
                workspace,
                request.scope_id,
                node.id(),
                source,
            )
            .await?;
        }
    }
    let next = locked.0.checked_add(1).ok_or(Error::StorageUnavailable)?;
    let review = SliceCandidateReview {
        revision: next,
        verdict: request.review.verdict,
        summary: request.review.summary.clone(),
        findings: request.review.findings.clone(),
    };
    sqlx::query("INSERT INTO slice_candidate_reviews(tenant_id,workspace_id,candidate_set_id,set_revision,payload) VALUES($1,$2,$3,$4,$5)").bind(tenant).bind(workspace).bind(request.candidate_set_id).bind(next).bind(json(&review)?).execute(&mut **tx).await.map_err(storage_error)?;
    let status = match request.review.verdict {
        SlicePlanReviewVerdict::Ready => "ready",
        SlicePlanReviewVerdict::Revise => "review_required",
        SlicePlanReviewVerdict::Blocked => "blocked",
    };
    sqlx::query("UPDATE slice_candidate_sets SET revision=$4,status=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.candidate_set_id).bind(next).bind(status).execute(&mut **tx).await.map_err(storage_error)?;
    let result = load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    save_receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "review_slice_set",
        request.request_id,
        payload,
        &result,
    )
    .await?;
    Ok(result)
}

fn require_review_status(status: &str) -> Result<()> {
    if status == "review_required" {
        Ok(())
    } else {
        Err(Error::refused(
            tect_domain::RefusalCode::ReviewRequired,
            "request_review",
            "review_required_candidate_set",
        ))
    }
}

pub(crate) async fn record_input(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &RecordSliceCandidateInput,
) -> Result<SliceCandidateContext> {
    let payload = json(request)?;
    if let Some(v) = receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "record_slice_input",
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
    if locked.0 != request.revision {
        return Err(Error::StaleRevision);
    }
    let next_input = locked.4.checked_add(1).ok_or(Error::StorageUnavailable)?;
    let next_rev = locked.0.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query("INSERT INTO slice_planning_inputs(tenant_id,workspace_id,candidate_set_id,sequence,request_id,session_id,input) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(tenant).bind(workspace).bind(request.candidate_set_id).bind(next_input).bind(request.request_id).bind(session).bind(&request.input).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_candidate_sets SET revision=$4,status='review_required',latest_input=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.candidate_set_id).bind(next_rev).bind(next_input).execute(&mut **tx).await.map_err(storage_error)?;
    let result = load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    save_receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "record_slice_input",
        request.request_id,
        payload,
        &result,
    )
    .await?;
    Ok(result)
}

pub(crate) async fn refresh(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &RefreshSliceCandidateSet,
    material: &SlicePlanningSnapshotMaterial,
) -> Result<SliceCandidateContext> {
    let payload = json(request)?;
    if let Some(v) = receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "refresh_slice_set",
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
    if locked.0 != request.revision {
        return Err(Error::StaleRevision);
    }
    let scope = load_scope(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::NotFound)?;
    let sequence:i64=sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0)+1 FROM slice_planning_snapshots WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3").bind(tenant).bind(workspace).bind(request.candidate_set_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let results:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3 ORDER BY created_at,id").bind(tenant).bind(workspace).bind(request.scope_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    insert_snapshot(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        request.scope_id,
        sequence,
        locked.4,
        scope.source_candidate_set_revision,
        scope.source_snapshot_id,
        material,
        &results,
    )
    .await?;
    let next = locked.0.checked_add(1).ok_or(Error::StorageUnavailable)?;
    let status = if load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .and_then(|c| c.draft)
        .is_some()
    {
        "review_required"
    } else {
        "draft"
    };
    sqlx::query("UPDATE slice_candidate_sets SET revision=$4,status=$5,input_cursor=latest_input WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.candidate_set_id).bind(next).bind(status).execute(&mut **tx).await.map_err(storage_error)?;
    let result = load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    save_receipt(
        tx,
        tenant,
        workspace,
        request.candidate_set_id,
        "refresh_slice_set",
        request.request_id,
        payload,
        &result,
    )
    .await?;
    Ok(result)
}

#[cfg(test)]
mod refusal_tests {
    use super::*;

    #[test]
    fn non_review_candidate_set_has_typed_review_refusal() {
        let error = require_review_status("draft").unwrap_err();
        assert_eq!(
            error.refusal().unwrap().code,
            tect_domain::RefusalCode::ReviewRequired
        );
    }
}
