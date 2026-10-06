use super::*;

pub(super) async fn authenticate(
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
    let role = decode_principal_role(&role)?;
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

pub(super) async fn set_tenant(store: &mut PgUnitOfWork, tenant_id: Uuid) -> Result<()> {
    let identity = store.identity.as_ref().ok_or(Error::Unauthorized)?;
    if identity.tenant_id != tenant_id {
        return Err(Error::Forbidden);
    }
    let value = tenant_id.to_string();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
        .bind(value)
        .execute(&mut **store.transaction()?)
        .await
        .map_err(storage_error)?;
    store.tenant_id = Some(tenant_id);
    Ok(())
}
