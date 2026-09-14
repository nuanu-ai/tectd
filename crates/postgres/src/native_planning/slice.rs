use super::*;

pub(crate) async fn open_slice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &OpenSlice,
) -> Result<OpenSliceOutcome> {
    let payload = json(request)?;
    if let Some((stored,result))=sqlx::query_as::<_,(serde_json::Value,Option<serde_json::Value>)>("SELECT origin_payload,origin_result FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND origin_request_id=$3").bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?{if stored!=payload{return Err(Error::InputConflict)}let prior:OpenSliceOutcome=decode(result.ok_or(Error::InternalInvariant)?)?;let slice=match prior{OpenSliceOutcome::Created(value)|OpenSliceOutcome::Replay(value)=>value};return Ok(OpenSliceOutcome::Replay(slice))}
    let scope_revision:i64=sqlx::query_scalar("SELECT revision FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE").bind(tenant).bind(workspace).bind(request.scope_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    let locked = lock_set(
        tx,
        tenant,
        workspace,
        request.scope_id,
        request.candidate_set_id,
    )
    .await?;
    if scope_revision != request.scope_revision || locked.0 != request.candidate_set_revision {
        return Err(Error::StaleRevision);
    }
    if locked.1 != "ready" || locked.2 != request.candidate_snapshot_id || locked.3 != locked.4 {
        return Err(Error::StaleContext);
    }
    let ctx = load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::NotFound)?;
    let draft = ctx.draft.ok_or(Error::InvalidArguments)?;
    let node = draft
        .nodes
        .iter()
        .find(|n| n.id() == request.candidate_id)
        .ok_or(Error::NotFound)?;
    if node.revision() != request.candidate_revision {
        return Err(Error::StaleRevision);
    }
    let (title, outcome, pipeline_kind, deps) = match node {
        SliceCandidateNode::Decision { .. } => return Err(Error::Forbidden),
        SliceCandidateNode::Work {
            title,
            outcome,
            pipeline,
            dependencies,
            ..
        } => (
            title.clone(),
            outcome.clone(),
            *pipeline,
            dependencies.clone(),
        ),
    };
    if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3 AND candidate_id=$4)").bind(tenant).bind(workspace).bind(request.scope_id).bind(request.candidate_id).fetch_one(&mut **tx).await.map_err(storage_error)?{return Err(Error::Forbidden)}
    for dep in deps {
        let Some(dep_node) = draft.nodes.iter().find(|n| n.id() == dep) else {
            return Err(Error::InvalidArguments);
        };
        if matches!(dep_node, SliceCandidateNode::Decision { .. }) {
            return Err(Error::Forbidden);
        }
        let state:Option<String>=sqlx::query_scalar("SELECT state FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3 AND candidate_id=$4").bind(tenant).bind(workspace).bind(request.scope_id).bind(dep).fetch_optional(&mut **tx).await.map_err(storage_error)?;
        if state.as_deref() != Some("completed") {
            return Err(Error::Forbidden);
        }
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO native_slices(id,tenant_id,workspace_id,scope_id,candidate_id,candidate_revision,opening_snapshot_id,title,outcome,pipeline,origin_request_id,origin_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)").bind(id).bind(tenant).bind(workspace).bind(request.scope_id).bind(request.candidate_id).bind(request.candidate_revision).bind(request.candidate_snapshot_id).bind(title).bind(outcome).bind(pipeline_kind.as_str()).bind(request.request_id).bind(&payload).execute(&mut **tx).await.map_err(storage_error)?;
    let slice = load_slice(tx, tenant, workspace, id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let outcome = OpenSliceOutcome::Created(slice);
    sqlx::query("UPDATE native_slices SET origin_result=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(id).bind(json(&outcome)?).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(outcome)
}

pub(crate) async fn record_result(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &RecordSliceResult,
) -> Result<RecordSliceResultOutcome> {
    let payload = json(request)?;
    if let Some((stored,result))=sqlx::query_as::<_,(serde_json::Value,Option<serde_json::Value>)>("SELECT request_payload,result_payload FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3").bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?{if stored!=payload{return Err(Error::InputConflict)}let prior:RecordSliceResultOutcome=decode(result.ok_or(Error::InternalInvariant)?)?;let (result,context)=match prior{RecordSliceResultOutcome::Created{result,context}|RecordSliceResultOutcome::Replay{result,context}=>(result,context)};return Ok(RecordSliceResultOutcome::Replay{result,context})}
    let scope:(i64,Uuid)=sqlx::query_as("SELECT revision,slice_candidate_set_id FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE").bind(tenant).bind(workspace).bind(request.scope_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    let row:(i64,String,String,Option<Uuid>,Option<Uuid>)=sqlx::query_as("SELECT revision,state,pipeline,knowledge_change_id,knowledge_run_id FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3 AND id=$4 FOR UPDATE").bind(tenant).bind(workspace).bind(request.scope_id).bind(request.slice_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    let managed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND slice_id=$3)")
        .bind(tenant).bind(workspace).bind(request.slice_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if managed
        || row.2 == PipelineKind::PromoteToDurableKnowledge.as_str()
        || row.3.is_some()
        || row.4.is_some()
    {
        return Err(Error::Forbidden);
    }
    if row.0 != request.slice_revision {
        return Err(Error::StaleRevision);
    }
    if row.1 == "completed" {
        return Err(Error::Forbidden);
    }
    let result_revision:i64=sqlx::query_scalar("SELECT COALESCE(MAX(revision),0)+1 FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND slice_id=$3").bind(tenant).bind(workspace).bind(request.slice_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let id = Uuid::new_v4();
    let outcome_text = match request.outcome {
        SliceResultOutcome::Completed => "completed",
        SliceResultOutcome::Blocked => "blocked",
    };
    sqlx::query("INSERT INTO slice_results(id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,revision,outcome,summary,evidence,scope_impact,remaining_work,request_id,request_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)").bind(id).bind(tenant).bind(workspace).bind(request.scope_id).bind(request.slice_id).bind(request.slice_revision).bind(result_revision).bind(outcome_text).bind(&request.summary).bind(json(&request.evidence)?).bind(&request.scope_impact).bind(&request.remaining_work).bind(request.request_id).bind(&payload).execute(&mut **tx).await.map_err(storage_error)?;
    let next_slice_revision = row.0.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query("UPDATE native_slices SET revision=$4,state=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.slice_id).bind(next_slice_revision).bind(outcome_text).execute(&mut **tx).await.map_err(storage_error)?;
    let set:(i64,i64)=sqlx::query_as("SELECT revision,latest_input FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE").bind(tenant).bind(workspace).bind(scope.1).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let next_input = set.1.checked_add(1).ok_or(Error::StorageUnavailable)?;
    let next_set_rev = set.0.checked_add(1).ok_or(Error::StorageUnavailable)?;
    let input = format!(
        "Externally reported Slice Result {}: {}",
        id, request.summary
    );
    sqlx::query("INSERT INTO slice_planning_inputs(tenant_id,workspace_id,candidate_set_id,sequence,session_id,source_result_id,input) VALUES($1,$2,$3,$4,$5,$6,$7)").bind(tenant).bind(workspace).bind(scope.1).bind(next_input).bind(session).bind(id).bind(input).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_candidate_sets SET revision=$4,status='review_required',latest_input=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(scope.1).bind(next_set_rev).bind(next_input).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE native_scopes SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(request.scope_id).execute(&mut **tx).await.map_err(storage_error)?;
    let result = SliceResult {
        id,
        slice_id: request.slice_id,
        slice_revision: request.slice_revision,
        revision: result_revision,
        outcome: request.outcome,
        summary: request.summary.clone(),
        evidence: request.evidence.clone(),
        scope_impact: request.scope_impact.clone(),
        remaining_work: request.remaining_work.clone(),
        provenance: "externally_reported".into(),
        pipeline_run_id: None,
        pipeline_definition_version: None,
        pipeline_definition_digest: None,
        pipeline_final_attempt_id: None,
        pipeline_result_origin: None,
        knowledge_provenance: None,
    };
    let mut context = load_context(tx, tenant, workspace, request.scope_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    context.stale_reasons = vec!["planning_inputs".into(), "slice_results".into()];
    let outcome = RecordSliceResultOutcome::Created { result, context };
    sqlx::query("UPDATE slice_results SET result_payload=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(id).bind(json(&outcome)?).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(outcome)
}
