use super::*;

pub(super) async fn grant_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
    runtime_role: &str,
) -> Result<()> {
    for statement in [
        format!("REVOKE ALL PRIVILEGES ON TABLE advisory_matrix_disposition FROM {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE advisory_matrix_disposition TO {quoted_role}"),
    ] {
        sqlx::query(&statement)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    let safe: bool = sqlx::query_scalar(
        "SELECT pg_catalog.has_table_privilege($1,'public.advisory_matrix_disposition','SELECT') \
            AND pg_catalog.has_table_privilege($1,'public.advisory_matrix_disposition','INSERT') \
            AND NOT pg_catalog.has_table_privilege($1,'public.advisory_matrix_disposition','UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')",
    ).bind(runtime_role).fetch_one(&mut **transaction).await.map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

pub(super) async fn validate_runtime_role(pool: &PgPool, runtime_role: &str) -> Result<()> {
    // Structural validation only, not proof of deployed predicate semantics.
    let safe: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname='advisory_matrix_disposition' \
           AND c.relkind='r' AND c.relrowsecurity AND c.relforcerowsecurity \
           AND NOT pg_catalog.pg_has_role($1,c.relowner,'MEMBER') \
           AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode( \
             COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) acl WHERE acl.grantee=0)) \
         AND (SELECT count(*)=1 AND COALESCE(bool_and( \
             polname='advisory_matrix_disposition_tenant_scope' AND polcmd='*' \
             AND polpermissive AND polroles=ARRAY[0::oid] AND polqual IS NOT NULL \
             AND polwithcheck IS NOT NULL \
             AND pg_catalog.pg_get_expr(polqual,polrelid)=pg_catalog.pg_get_expr(polwithcheck,polrelid)),false) \
             FROM pg_catalog.pg_policy WHERE polrelid='public.advisory_matrix_disposition'::regclass) \
         AND EXISTS(SELECT 1 FROM pg_catalog.pg_trigger t \
         JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid \
         JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
         WHERE t.tgrelid='public.advisory_matrix_disposition'::regclass \
           AND t.tgname='advisory_matrix_disposition_active_owner' \
           AND NOT t.tgisinternal AND t.tgenabled='O' AND t.tgtype=7 \
           AND t.tgnargs=0 AND pg_catalog.octet_length(t.tgargs)=0 \
           AND n.nspname='public' AND p.proname='matrix_disposition_require_active_owner' \
           AND p.pronargs=0 AND p.prorettype='pg_catalog.trigger'::regtype AND p.prosecdef \
           AND EXISTS(SELECT 1 FROM pg_catalog.unnest(p.proconfig) cfg \
              WHERE pg_catalog.replace(cfg,' ','')='search_path=pg_catalog,public') \
           AND NOT pg_catalog.pg_has_role($1,p.proowner,'MEMBER') \
           AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE') \
           AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode( \
              COALESCE(p.proacl,pg_catalog.acldefault('f',p.proowner))) acl \
              WHERE acl.grantee=0 AND acl.privilege_type='EXECUTE'))",
    ).bind(runtime_role).fetch_one(pool).await.map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
