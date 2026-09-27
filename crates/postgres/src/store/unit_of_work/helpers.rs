use super::*;

pub(super) async fn workspace_by_key(
    store: &mut PgUnitOfWork,
    key: &str,
) -> Result<Option<Workspace>> {
    let tenant_id = store.tenant_id()?;
    let row: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, key FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(tenant_id)
            .bind(key)
            .fetch_optional(&mut **store.transaction()?)
            .await
            .map_err(storage_error)?;
    Ok(row.map(|(id, key)| Workspace { id, key }))
}

pub(super) async fn authenticate_host(
    store: &mut PgUnitOfWork,
    auth: &HostAuth,
) -> Result<HostIdentity> {
    let digest = runtime::credential_digest(&auth.credential);
    let for_write = store.mode == TransactionMode::ReadWrite;
    let row: Option<(Uuid, Uuid, String, serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT tenant_id, principal_id, principal_role, allowed_source_roots, allowed_setup_roots \
             FROM public.tect_authenticate_host($1, $2, $3)",
    )
    .bind(auth.host_id)
    .bind(digest)
    .bind(for_write)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    let (tenant_id, principal_id, role, source_roots, setup_roots) =
        row.ok_or(Error::Unauthorized)?;
    let role = match role.as_str() {
        "owner" => PrincipalRole::Owner,
        "verifier" => PrincipalRole::Verifier,
        _ => return Err(Error::Unauthorized),
    };
    let allowed_source_roots = serde_json::from_value(source_roots).map_err(storage_error)?;
    let allowed_setup_roots = serde_json::from_value(setup_roots).map_err(storage_error)?;
    let identity = HostIdentity {
        host_id: auth.host_id,
        tenant_id,
        principal_id,
        role,
        allowed_source_roots,
        allowed_setup_roots,
    };
    store.identity = Some(identity.clone());
    Ok(identity)
}

pub(super) async fn ensure_session_record(
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
