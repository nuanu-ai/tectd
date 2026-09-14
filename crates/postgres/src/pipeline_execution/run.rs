use super::*;

pub(crate) async fn begin_replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    request: &BeginPipelineRun,
) -> Result<Option<BeginPipelineRunOutcome>> {
    let payload = json(request)?;
    let row:Option<(Uuid,Option<serde_json::Value>,Option<serde_json::Value>,bool)>=sqlx::query_as(
        "SELECT id,origin_payload,origin_result,payload_erased FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND origin_request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((run_id, stored, result, erased)) = row else {
        return Ok(None);
    };
    if erased {
        context::authorize_run_origin(tx, tenant, workspace, principal, run_id).await?;
        return Err(Error::KnowledgePayloadErased);
    }
    if stored != Some(payload) {
        return Err(Error::InputConflict);
    }
    let result = result.ok_or(Error::InternalInvariant)?;
    if let Some(manifest) = origin_manifest_id(&result)? {
        crate::durable_knowledge::manifest::authorize_manifest(
            tx, tenant, workspace, manifest, principal,
        )
        .await?
        .ok_or(Error::Forbidden)?;
    }
    let prior: BeginPipelineRunOutcome = decode(result)?;
    let context = match prior {
        BeginPipelineRunOutcome::Created(value) | BeginPipelineRunOutcome::Replay(value) => value,
    };
    Ok(Some(BeginPipelineRunOutcome::Replay(context)))
}

fn origin_manifest_id(value: &serde_json::Value) -> Result<Option<Uuid>> {
    for outcome in ["created", "replay"] {
        for field in ["knowledge_resources", "knowledge"] {
            if let Some(value) = value
                .get(outcome)
                .and_then(|context| context.get(field))
                .and_then(|manifest| manifest.get("id"))
                .and_then(serde_json::Value::as_str)
            {
                return Uuid::parse_str(value)
                    .map(Some)
                    .map_err(|_| Error::InternalInvariant);
            }
        }
    }
    Ok(None)
}

pub(crate) async fn begin(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &BeginPipelineRun,
    definition: &PipelineDefinitionSnapshot,
) -> Result<BeginPipelineRunOutcome> {
    // DK lock order: workspace knowledge state precedes Slice/run locks.
    let _ = crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let principal = session_principal(tx, session).await?;
    let row:Option<(Uuid,i64,String,String)>=sqlx::query_as(
        "SELECT scope_id,revision,pipeline,state FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.slice_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let (scope, revision, kind, state) = row.ok_or(Error::NotFound)?;
    if let Some(replay) = begin_replay(tx, tenant, workspace, principal, request).await? {
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
    let origin_manifest = if let Some(manifest) = crate::durable_knowledge::manifest::capture(
        tx,
        tenant,
        workspace,
        id,
        1,
        scope,
        request.slice_id,
        &first.id,
        session,
    )
    .await?
    {
        sqlx::query("UPDATE slice_pipeline_runs SET knowledge_manifest_id=$4,knowledge_manifest_digest=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(id).bind(manifest.id).bind(&manifest.digest).execute(&mut **tx).await.map_err(storage_error)?;
        Some(manifest.id)
    } else {
        None
    };
    let context = load_context(tx, tenant, workspace, principal, id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let outcome = BeginPipelineRunOutcome::Created(context);
    sqlx::query("UPDATE slice_pipeline_runs SET origin_result=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(id).bind(json(&outcome)?).execute(&mut **tx).await.map_err(storage_error)?;
    if let Some(manifest) = origin_manifest {
        crate::knowledge_lifecycle::erase::register_pipeline_run_origin_copies(
            tx, tenant, workspace, id, manifest,
        )
        .await?;
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_origin_manifest_id_supports_legacy_and_generic_contexts() {
        let legacy = Uuid::new_v4();
        let generic = Uuid::new_v4();
        assert_eq!(
            origin_manifest_id(&serde_json::json!({"created":{"knowledge":{"id":legacy}}}))
                .unwrap(),
            Some(legacy)
        );
        assert_eq!(
            origin_manifest_id(
                &serde_json::json!({"replay":{"knowledge_resources":{"id":generic}}})
            )
            .unwrap(),
            Some(generic)
        );
        assert_eq!(
            origin_manifest_id(&serde_json::json!({"created":{"knowledge":null}})).unwrap(),
            None
        );
    }
}
