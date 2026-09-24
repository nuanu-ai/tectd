use super::*;

const MATRIX_ADVISORY_TABLES: [&str; 3] = [
    "advisory_matrix_advice",
    "advisory_matrix_disposition",
    "matrix_planning_selection_links",
];

pub(super) async fn grant_matrix_advisory_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
) -> Result<()> {
    let tables = MATRIX_ADVISORY_TABLES.join(",");
    for statement in [
        format!("REVOKE ALL PRIVILEGES ON TABLE {tables} FROM {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE {tables} TO {quoted_role}"),
    ] {
        sqlx::query(&statement)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) async fn validate_matrix_advisory_schema(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runtime_role: &str,
) -> Result<()> {
    let tables_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=3 AND pg_catalog.bool_and( \
             c.relrowsecurity AND c.relforcerowsecurity \
             AND NOT pg_catalog.pg_has_role(r.oid,c.relowner,'MEMBER')) \
         FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         JOIN pg_catalog.pg_roles r ON r.rolname=$1 \
         WHERE n.nspname='public' AND c.relkind='r' AND c.relname=ANY($2)",
    )
    .bind(runtime_role)
    .bind(MATRIX_ADVISORY_TABLES)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let policies_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=3 AND pg_catalog.bool_and(COALESCE( \
             p.policyname=p.tablename||'_tenant_scope' \
             AND p.permissive='PERMISSIVE' AND p.roles='{public}'::name[] \
             AND p.cmd='ALL' AND p.qual=p.with_check \
             AND pg_catalog.regexp_replace(p.qual,'[[:space:]]+','','g') \
                 =pg_catalog.regexp_replace(pg_catalog.format( \
                    '((CURRENT_USER = pg_get_userbyid(( SELECT pg_class.relowner FROM pg_class WHERE (pg_class.oid = (%L::regclass)::oid)))) OR (tenant_id = (NULLIF(current_setting(''tect.tenant_id''::text, true), ''''::text))::uuid))', \
                    p.tablename),'[[:space:]]+','','g'),false)) \
         FROM pg_catalog.pg_policies p \
         WHERE p.schemaname='public' AND p.tablename=ANY($1)",
    )
    .bind(MATRIX_ADVISORY_TABLES)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let public_revoked: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS( \
             SELECT 1 FROM pg_catalog.pg_class c \
             CROSS JOIN LATERAL pg_catalog.aclexplode( \
                 COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) acl \
             JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname='public' AND c.relname=ANY($1) AND acl.grantee=0)",
    )
    .bind(MATRIX_ADVISORY_TABLES)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let runtime_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.bool_and( \
             pg_catalog.has_table_privilege($1,pg_catalog.format('public.%I',table_name),'SELECT') \
             AND pg_catalog.has_table_privilege($1,pg_catalog.format('public.%I',table_name),'INSERT') \
             AND NOT pg_catalog.has_table_privilege($1,pg_catalog.format('public.%I',table_name),'UPDATE') \
             AND NOT pg_catalog.has_table_privilege($1,pg_catalog.format('public.%I',table_name),'DELETE') \
             AND NOT pg_catalog.has_table_privilege($1,pg_catalog.format('public.%I',table_name),'TRUNCATE') \
             AND NOT pg_catalog.has_table_privilege($1,pg_catalog.format('public.%I',table_name),'REFERENCES') \
             AND NOT pg_catalog.has_table_privilege($1,pg_catalog.format('public.%I',table_name),'TRIGGER')) \
         FROM pg_catalog.unnest($2::text[]) table_name",
    )
    .bind(runtime_role)
    .bind(MATRIX_ADVISORY_TABLES)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let disposition_guard_ready: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_trigger t \
         JOIN pg_catalog.pg_class c ON c.oid=t.tgrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid \
         JOIN pg_catalog.pg_roles r ON r.rolname=$1 \
         WHERE n.nspname='public' AND c.relname='advisory_matrix_disposition' \
           AND t.tgname='advisory_matrix_disposition_active_owner' \
           AND t.tgenabled='O' AND NOT t.tgisinternal \
           AND p.proname='matrix_disposition_require_active_owner' \
           AND p.prosecdef AND NOT pg_catalog.pg_has_role(r.oid,p.proowner,'MEMBER') \
           AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE'))",
    )
    .bind(runtime_role)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let selection_guard_ready: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_trigger t \
         JOIN pg_catalog.pg_class c ON c.oid=t.tgrelid \
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid \
         JOIN pg_catalog.pg_roles r ON r.rolname=$1 \
         WHERE n.nspname='public' AND c.relname='matrix_planning_selection_links' \
           AND t.tgname='matrix_planning_selection_active_owner' \
           AND t.tgenabled='O' AND NOT t.tgisinternal \
           AND p.proname='matrix_planning_selection_require_active_owner' \
           AND p.prosecdef AND NOT pg_catalog.pg_has_role(r.oid,p.proowner,'MEMBER') \
           AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE'))",
    )
    .bind(runtime_role)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if !tables_ready
        || !policies_ready
        || !public_revoked
        || !runtime_ready
        || !disposition_guard_ready
        || !selection_guard_ready
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
