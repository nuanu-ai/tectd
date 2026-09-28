use super::*;

pub(super) async fn read(
    store: &mut PgUnitOfWork,
    workspace_id: Uuid,
    session_id: Uuid,
) -> Result<SessionAdvisoryPreference> {
    let tenant_id = store.tenant_id()?;
    let row: Option<(String, i64)> = sqlx::query_as(
        "SELECT advisory_preference,advisory_preference_revision FROM agent_sessions \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT revoked",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(session_id)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    let (value, revision) = row.ok_or(Error::Forbidden)?;
    let preference = match value.as_str() {
        "use_workspace" => AdvisoryRequestPreference::UseWorkspace,
        "skip" => AdvisoryRequestPreference::Skip,
        _ => return Err(Error::InternalInvariant),
    };
    Ok(SessionAdvisoryPreference {
        preference,
        revision,
    })
}

pub(super) async fn set(
    store: &mut PgUnitOfWork,
    workspace_id: Uuid,
    session_id: Uuid,
    principal_id: Uuid,
    request: &SetSessionAdvisoryPreference,
) -> Result<SessionAdvisoryPreference> {
    request.validate()?;
    let tenant_id = store.tenant_id()?;
    let current: Option<(String, i64)> = sqlx::query_as(
        "SELECT advisory_preference,advisory_preference_revision FROM agent_sessions \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT revoked FOR UPDATE",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(session_id)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    let (old_value, revision) = current.ok_or(Error::Forbidden)?;
    if revision != request.expected_revision {
        return Err(Error::StaleRevision);
    }
    let next = revision.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query(
        "INSERT INTO session_advisory_preference_history \
         (tenant_id,workspace_id,session_id,revision,previous_revision,preference,changed_by_principal_id) \
         VALUES ($1,$2,$3,0,NULL,$4,$5) ON CONFLICT DO NOTHING",
    )
    .bind(tenant_id).bind(workspace_id).bind(session_id).bind(&old_value).bind(principal_id)
    .execute(&mut **store.transaction()?).await.map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO session_advisory_preference_history \
         (tenant_id,workspace_id,session_id,revision,previous_revision,preference,changed_by_principal_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(tenant_id).bind(workspace_id).bind(session_id).bind(next).bind(revision)
    .bind(request.preference.as_str()).bind(principal_id)
    .execute(&mut **store.transaction()?).await.map_err(storage_error)?;
    let changed = sqlx::query(
        "UPDATE agent_sessions SET advisory_preference=$4,advisory_preference_revision=$5 \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT revoked AND advisory_preference_revision=$6",
    )
    .bind(tenant_id).bind(workspace_id).bind(session_id).bind(request.preference.as_str())
    .bind(next).bind(revision)
    .execute(&mut **store.transaction()?).await.map_err(storage_error)?;
    if changed.rows_affected() != 1 {
        return Err(Error::StaleRevision);
    }
    Ok(SessionAdvisoryPreference {
        preference: request.preference,
        revision: next,
    })
}
