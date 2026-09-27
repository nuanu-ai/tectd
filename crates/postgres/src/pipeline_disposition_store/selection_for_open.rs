use super::*;

/// Resolve a captured planning choice inside the same transaction that opens
/// the Slice. The currentness read locks the Work/Matrix/source basis until the
/// open insert; no provider content or caller-supplied kind is trusted here.
pub(crate) async fn selection_for_open(
    store: &mut PgUnitOfWork,
    workspace_id: Uuid,
    session_id: Uuid,
    request: &OpenSlice,
    replay: bool,
) -> Result<PipelineKind> {
    let disposition_id = request.disposition_id.ok_or(Error::InvalidArguments)?;
    let tenant = store.tenant_id()?;
    let actor_id = store.principal_id()?;
    let row = sqlx::query(
        "SELECT result_payload,opportunity_id,actor_id,session_id,work_node_id,\
                work_node_revision,manifest_digest,matrix_disposition_id,source_snapshot_id, \
                selected_option_id,verification_plan_id,verification_plan_version, \
                verification_plan_digest,verification_plan_source_definition_digest \
         FROM pipeline_advice_dispositions WHERE tenant_id=$1 AND workspace_id=$2 \
           AND disposition_id=$3",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(disposition_id)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    let result: PipelineDispositionResult = serde_json::from_value(
        row.try_get::<Value, _>("result_payload")
            .map_err(storage_error)?,
    )
    .map_err(|_| Error::InputConflict)?;
    let opportunity_id: Uuid = row.try_get("opportunity_id").map_err(storage_error)?;
    if result.id != disposition_id
        || result.request.opportunity_id != opportunity_id
        || row.try_get::<Uuid, _>("actor_id").map_err(storage_error)? != actor_id
        || row
            .try_get::<Uuid, _>("session_id")
            .map_err(storage_error)?
            != session_id
        || row
            .try_get::<Uuid, _>("work_node_id")
            .map_err(storage_error)?
            != request.candidate_id
        || row
            .try_get::<i64, _>("work_node_revision")
            .map_err(storage_error)?
            != request.candidate_revision
    {
        return Err(Error::Forbidden);
    }
    let selected = result.selected_kind.ok_or(Error::Forbidden)?;
    let selected_option_id = result
        .selected_option_id
        .as_deref()
        .ok_or(Error::Forbidden)?;
    let persisted_option_id: Option<String> =
        row.try_get("selected_option_id").map_err(storage_error)?;
    let plan_id: Option<String> = row.try_get("verification_plan_id").map_err(storage_error)?;
    let plan_version: Option<String> = row
        .try_get("verification_plan_version")
        .map_err(storage_error)?;
    let plan_digest: Option<String> = row
        .try_get("verification_plan_digest")
        .map_err(storage_error)?;
    let source_definition_digest: Option<String> = row
        .try_get("verification_plan_source_definition_digest")
        .map_err(storage_error)?;
    if persisted_option_id.as_deref() != Some(selected_option_id)
        || plan_id.as_deref() != selected_option_id.split_once('+').map(|(_, id)| id)
        || !selected_option_id.starts_with(&format!("{}+", selected.as_str()))
        || plan_digest.as_deref()
            != plan_id
                .as_deref()
                .and_then(|id| id.strip_prefix("verification-plan:"))
        || plan_version.as_deref().is_none_or(str::is_empty)
        || source_definition_digest
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return Err(Error::InputConflict);
    }
    if replay {
        return Ok(selected);
    }
    let basis = load_basis(store, workspace_id, opportunity_id)
        .await?
        .ok_or(Error::StaleContext)?;
    let context = &basis.prepared.context;
    let manifest = &basis.prepared.manifest;
    let opportunity = &basis.prepared.opportunity;
    if !is_current(store, workspace_id, &basis).await?
        || opportunity.authorized_actor_id != actor_id
        || opportunity.session_id != session_id
        || context.scope_id != request.scope_id
        || context.candidate_set_id != request.candidate_set_id
        || context.candidate_set_revision != request.candidate_set_revision
        || context.planning_snapshot_id != request.candidate_snapshot_id
        || context.work_node_id != request.candidate_id
        || context.work_node_revision != request.candidate_revision
        || context.matrix_disposition_id
            != row
                .try_get::<Uuid, _>("matrix_disposition_id")
                .map_err(storage_error)?
        || context.source_snapshot_id
            != row
                .try_get::<Uuid, _>("source_snapshot_id")
                .map_err(storage_error)?
        || manifest.digest
            != row
                .try_get::<String, _>("manifest_digest")
                .map_err(storage_error)?
        || result
            .request
            .resolve(result.id, manifest, &basis.saved_work, &basis.advice)?
            != result
        || !manifest.options.iter().any(|option| {
            option.id == selected_option_id
                && option.kind == selected
                && Some(option.verification_plan.id.as_str()) == plan_id.as_deref()
                && Some(option.verification_plan.digest.as_str()) == plan_digest.as_deref()
                && Some(option.verification_plan.source_definition_version.as_str())
                    == plan_version.as_deref()
                && Some(option.verification_plan.source_definition_digest.as_str())
                    == source_definition_digest.as_deref()
        })
    {
        return Err(Error::StaleContext);
    }
    Ok(selected)
}
