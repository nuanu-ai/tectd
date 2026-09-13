use super::*;

pub(crate) async fn begin_replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &BeginPipelineRun,
) -> Result<Option<BeginPipelineRunOutcome>> {
    let payload = json(request)?;
    let row:Option<(Uuid,serde_json::Value,Option<serde_json::Value>)>=sqlx::query_as(
        "SELECT id,origin_payload,origin_result FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND origin_request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((_, stored, result)) = row else {
        return Ok(None);
    };
    if stored != payload {
        return Err(Error::InputConflict);
    }
    let prior: BeginPipelineRunOutcome = decode(result.ok_or(Error::InternalInvariant)?)?;
    let context = match prior {
        BeginPipelineRunOutcome::Created(value) | BeginPipelineRunOutcome::Replay(value) => value,
    };
    Ok(Some(BeginPipelineRunOutcome::Replay(context)))
}

pub(crate) async fn begin(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &BeginPipelineRun,
    definition: &PipelineDefinitionSnapshot,
) -> Result<BeginPipelineRunOutcome> {
    let row:Option<(Uuid,i64,String,String)>=sqlx::query_as(
        "SELECT scope_id,revision,pipeline,state FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.slice_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let (scope, revision, kind, state) = row.ok_or(Error::NotFound)?;
    if let Some(replay) = begin_replay(tx, tenant, workspace, request).await? {
        return Ok(replay);
    }
    if scope != request.scope_id || revision != request.slice_revision {
        return Err(Error::StaleRevision);
    }
    if state != "open" || pipeline(&kind)? != definition.kind {
        return Err(Error::Forbidden);
    }
    if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND slice_id=$3)")
        .bind(tenant).bind(workspace).bind(request.slice_id).fetch_one(&mut **tx).await.map_err(storage_error)? { return Err(Error::Forbidden) }
    let first = definition.phases.first().ok_or(Error::InternalInvariant)?;
    let selected_mode = request.delivery_mode.unwrap_or(definition.default_mode);
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO slice_pipeline_runs(id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,definition_kind,definition_version,definition_digest,definition,delivery_mode,qualification_reason,current_phase_id,current_phase_ordinal,origin_request_id,origin_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)")
        .bind(id).bind(tenant).bind(workspace).bind(scope).bind(request.slice_id).bind(revision)
        .bind(definition.kind.as_str()).bind(&definition.version).bind(&definition.digest).bind(json(definition)?)
        .bind(enum_text(&selected_mode)?).bind(&request.qualification_reason).bind(&first.id).bind(first.ordinal as i32)
        .bind(request.request_id).bind(json(request)?).execute(&mut **tx).await.map_err(storage_error)?;
    let context = load_context(tx, tenant, workspace, id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let outcome = BeginPipelineRunOutcome::Created(context);
    sqlx::query("UPDATE slice_pipeline_runs SET origin_result=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(id).bind(json(&outcome)?).execute(&mut **tx).await.map_err(storage_error)?;
    let _ = session;
    Ok(outcome)
}
