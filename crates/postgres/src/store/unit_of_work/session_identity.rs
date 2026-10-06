use super::*;

pub(super) async fn ensure(
    store: &mut PgUnitOfWork,
    host_id: Uuid,
    workspace_id: Uuid,
    native_id: &str,
) -> Result<Created<Session>> {
    let tenant_id = store.tenant_id()?;
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO agent_sessions \
                 (id, tenant_id, host_id, workspace_id, native_session_id) \
             VALUES (pg_catalog.gen_random_uuid(), $1, $2, $3, $4) \
             ON CONFLICT (host_id, native_session_id) DO NOTHING RETURNING id",
    )
    .bind(tenant_id)
    .bind(host_id)
    .bind(workspace_id)
    .bind(native_id)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    let row: (Uuid, Uuid, Uuid, String, bool) = sqlx::query_as(
        "SELECT id, workspace_id, host_id, native_session_id, revoked \
             FROM agent_sessions \
             WHERE tenant_id=$1 AND host_id=$2 AND native_session_id=$3",
    )
    .bind(tenant_id)
    .bind(host_id)
    .bind(native_id)
    .fetch_one(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    Ok(Created {
        value: Session {
            id: row.0,
            workspace_id: row.1,
            host_id: row.2,
            native_session_id: row.3,
            revoked: row.4,
        },
        created: inserted.is_some(),
    })
}
