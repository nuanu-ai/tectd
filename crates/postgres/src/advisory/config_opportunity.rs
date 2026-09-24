async fn config(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
) -> Result<WorkspaceAdvisoryConfig> {
    let row: Option<(i64, String, Option<String>, Option<serde_json::Value>)> = sqlx::query_as(
        "SELECT revision,mode,provider_profile_ref,model_configuration FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    match row {
        Some((revision, value, provider_profile_ref, model_configuration)) => {
            Ok(WorkspaceAdvisoryConfig {
                workspace_id: workspace,
                revision,
                mode: mode(&value)?,
                materialized: true,
                provider_profile_ref: provider_profile_ref
                    .map(|id| AdvisoryProviderProfileRef { id }),
                model_configuration: model_configuration
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(storage_error)?,
            })
        }
        None => Ok(WorkspaceAdvisoryConfig {
            workspace_id: workspace,
            revision: ADVISORY_CONFIG_REVISION_DEFAULT,
            mode: WorkspaceAdvisoryMode::Disabled,
            materialized: false,
            provider_profile_ref: None,
            model_configuration: None,
        }),
    }
}

async fn configure(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &ConfigureWorkspaceAdvisory,
) -> Result<WorkspaceAdvisoryConfig> {
    request.validate()?;
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if !owner {
        return Err(Error::Forbidden);
    }

    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(workspace).bind(principal).bind(session).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,0,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(workspace).bind(principal).bind(session).execute(&mut **tx).await.map_err(storage_error)?;

    let current: i64 = sqlx::query_scalar("SELECT revision FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE")
        .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if current != request.expected_revision {
        return Err(Error::StaleRevision);
    }
    let next_revision = current.checked_add(1).ok_or(Error::StorageUnavailable)?;
    let provider_profile_ref = request.provider_profile_ref.as_ref().map(|value| &value.id);
    let model_configuration = request
        .model_configuration
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(storage_error)?;

    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(tenant).bind(workspace).bind(next_revision).bind(current).bind(request.mode.as_str()).bind(provider_profile_ref).bind(&model_configuration).bind(principal).bind(session).execute(&mut **tx).await.map_err(storage_error)?;
    let updated = sqlx::query("UPDATE advisory_workspace_config SET revision=$3,mode=$4,provider_profile_ref=$5,model_configuration=$6,updated_by_principal_id=$7,updated_by_session_id=$8,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND revision=$9")
        .bind(tenant).bind(workspace).bind(next_revision).bind(request.mode.as_str()).bind(provider_profile_ref).bind(&model_configuration).bind(principal).bind(session).bind(current).execute(&mut **tx).await.map_err(storage_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::StaleRevision);
    }
    Ok(WorkspaceAdvisoryConfig {
        workspace_id: workspace,
        revision: next_revision,
        mode: request.mode,
        materialized: true,
        provider_profile_ref: request.provider_profile_ref.clone(),
        model_configuration: request.model_configuration.clone(),
    })
}

async fn materialize_config(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
) -> Result<WorkspaceAdvisoryConfig> {
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(workspace).bind(principal).bind(session).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,0,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(workspace).bind(principal).bind(session).execute(&mut **tx).await.map_err(storage_error)?;
    let row: (i64, String, Option<String>, Option<serde_json::Value>) = sqlx::query_as(
        "SELECT revision,mode,provider_profile_ref,model_configuration FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(tenant).bind(workspace).fetch_one(&mut **tx).await.map_err(storage_error)?;
    Ok(WorkspaceAdvisoryConfig {
        workspace_id: workspace,
        revision: row.0,
        mode: mode(&row.1)?,
        materialized: true,
        provider_profile_ref: row.2.map(|id| AdvisoryProviderProfileRef { id }),
        model_configuration: row
            .3
            .map(serde_json::from_value)
            .transpose()
            .map_err(storage_error)?,
    })
}

fn opportunity_from_row(workspace: Uuid, row: OpportunityRow) -> Result<AdvisoryOpportunity> {
    let work_revision = row
        .source_revision
        .map(|value| value.parse::<i64>().map_err(storage_error))
        .transpose()?;
    Ok(AdvisoryOpportunity {
        id: row.id,
        workspace_id: workspace,
        session_id: row.session_id,
        authorized_actor_id: row.authorized_actor_id,
        capability: capability(&row.capability)?,
        decision_point: decision_point(&row.decision_point)?,
        decision_point_version: ADVISORY_DECISION_POINT_VERSION,
        workflow_occurrence_key: row.request_key,
        target_kind: row.work_item_kind,
        target_id: row.work_item_id,
        work_revision,
        matrix_task_revision: row.matrix_task_revision,
        matrix_choice_set_digest: row.matrix_choice_set_digest,
        matrix_verification_digest: row.matrix_verification_digest,
        source_ref: None,
        session_preference: preference(&row.session_preference)?,
        request_preference: preference(&row.request_preference)?,
        config_revision: row.config_revision,
        material_digest: row.material_digest,
        state: opportunity_state(&row.state)?,
        primary_reason: reason(&row.primary_reason)?,
        provider_called: false,
    })
}

async fn capture_opportunity(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    input: &AdvisoryOpportunityInput,
) -> Result<AdvisoryOpportunity> {
    input.validate()?;
    sqlx::query(
        "INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(input.authorized_actor_id)
    .bind(input.session_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,0,'disabled',NULL,NULL,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(input.authorized_actor_id)
    .bind(input.session_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    let current_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if current_revision != input.config_revision {
        return Err(Error::StaleRevision);
    }
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,matrix_task_revision,matrix_choice_set_digest,matrix_verification_digest,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21) ON CONFLICT(tenant_id,workspace_id,request_key) DO NOTHING")
        .bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(&input.target_kind).bind(input.target_id).bind(input.session_id).bind(input.authorized_actor_id).bind(input.work_revision.map(|value| value.to_string())).bind(input.matrix_task_revision).bind(&input.matrix_choice_set_digest).bind(&input.matrix_verification_digest).bind(input.capability.as_str()).bind(input.decision_point.as_str()).bind(input.config_revision).bind(input.session_preference.as_str()).bind(input.request_preference.as_str()).bind(ADVISORY_POLICY_VERSION).bind(&input.workflow_occurrence_key).bind(&input.material_digest).bind(input.state.as_str()).bind(input.primary_reason.as_str()).execute(&mut **tx).await.map_err(storage_error)?;
    let row: OpportunityRow = sqlx::query_as("SELECT id,session_id,authorized_actor_id,work_item_kind,work_item_id,source_revision,matrix_task_revision,matrix_choice_set_digest,matrix_verification_digest,capability,decision_point,config_revision,session_preference,request_preference,request_key,material_digest,state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND request_key=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(&input.workflow_occurrence_key).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if row.material_digest != input.material_digest
        || row.session_id != input.session_id
        || row.authorized_actor_id != input.authorized_actor_id
        || row.work_item_kind != input.target_kind
        || row.work_item_id != input.target_id
        || row.matrix_task_revision != input.matrix_task_revision
        || row.matrix_choice_set_digest != input.matrix_choice_set_digest
        || row.matrix_verification_digest != input.matrix_verification_digest
    {
        return Err(Error::InputConflict);
    }
    opportunity_from_row(workspace, row)
}

async fn opportunity_by_id(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity_id: Uuid,
    for_update: bool,
) -> Result<AdvisoryOpportunity> {
    let suffix = if for_update { " FOR UPDATE" } else { "" };
    let sql = format!(
        "SELECT id,session_id,authorized_actor_id,work_item_kind,work_item_id,source_revision,matrix_task_revision,matrix_choice_set_digest,matrix_verification_digest,capability,decision_point,config_revision,session_preference,request_preference,request_key,material_digest,state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3{suffix}"
    );
    let row: OpportunityRow = sqlx::query_as(&sql)
        .bind(tenant)
        .bind(workspace)
        .bind(opportunity_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?
        .ok_or(Error::NotFound)?;
    opportunity_from_row(workspace, row)
}

async fn opportunity_by_request_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request_key: &str,
) -> Result<Option<AdvisoryOpportunity>> {
    let row: Option<OpportunityRow> = sqlx::query_as(
        "SELECT id,session_id,authorized_actor_id,work_item_kind,work_item_id,source_revision,matrix_task_revision,matrix_choice_set_digest,matrix_verification_digest,capability,decision_point,config_revision,session_preference,request_preference,request_key,material_digest,state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND request_key=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    row.map(|value| opportunity_from_row(workspace, value))
        .transpose()
}
