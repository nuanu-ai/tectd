use super::*;

const TABLES: &str = "'advisory_matrix_advice','advisory_provider_observations', \
    'advisory_budget_policies','advisory_budget_reservations', \
    'advisory_budget_consumptions','matrix_v1_dispatch_cutover_allowlist'";
const FUNCTIONS: &str = "'advisory_dispatch_require_matrix_choice', \
    'advisory_budget_policy_guard','advisory_budget_reservation_guard', \
    'advisory_budget_consumption_guard','advisory_budget_consumption_exhaustion_guard', \
    'advisory_budget_policy_usage_totals','advisory_budget_global_reservation_guard', \
    'advisory_budget_global_consumption_guard','advisory_provider_observation_guard', \
    'matrix_v1_cutover_allowlist_immutable','matrix_v1_cutover_advice_insert_guard', \
    'matrix_v1_cutover_fingerprint'";

pub(super) async fn validate_runtime_role(pool: &PgPool, runtime_role: &str) -> Result<()> {
    let query = format!(
        "SELECT count(*)::bigint,COALESCE(bool_and(c.relrowsecurity AND c.relforcerowsecurity \
         AND NOT pg_catalog.pg_has_role($1,c.relowner,'MEMBER')),false) \
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relkind='r' AND c.relname IN ({TABLES})"
    );
    let (count, safe): (i64, bool) = sqlx::query_as(&query)
        .bind(runtime_role)
        .fetch_one(pool)
        .await
        .map_err(storage_error)?;
    if count != 6 || !safe {
        return Err(Error::InvalidConfiguration);
    }
    let query = format!(
        "SELECT count(*)::bigint,COALESCE(bool_and( \
         NOT pg_catalog.pg_has_role($1,p.proowner,'MEMBER') \
         AND p.pronargs=CASE WHEN p.proname='advisory_budget_policy_usage_totals' THEN 5 \
                            WHEN p.proname='matrix_v1_cutover_fingerprint' THEN 4 ELSE 0 END \
         AND EXISTS(SELECT 1 FROM pg_catalog.unnest(p.proconfig) cfg \
                    WHERE pg_catalog.replace(cfg,' ','')='search_path=pg_catalog,public') \
         AND (p.proname='matrix_v1_cutover_fingerprint' OR \
             (NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE') AND NOT EXISTS( \
              SELECT 1 FROM pg_catalog.aclexplode(COALESCE(p.proacl,pg_catalog.acldefault('f',p.proowner))) acl \
              WHERE acl.grantee=0 AND acl.privilege_type='EXECUTE'))) \
         AND p.prosecdef=(p.proname IN ('advisory_budget_policy_guard', \
              'advisory_budget_reservation_guard','advisory_budget_consumption_guard', \
              'advisory_budget_consumption_exhaustion_guard','advisory_budget_policy_usage_totals', \
              'advisory_budget_global_reservation_guard','advisory_budget_global_consumption_guard'))),false) \
         FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
         WHERE n.nspname='public' AND p.proname IN ({FUNCTIONS})"
    );
    let (count, safe): (i64, bool) = sqlx::query_as(&query)
        .bind(runtime_role)
        .fetch_one(pool)
        .await
        .map_err(storage_error)?;
    if count != 12 || !safe {
        return Err(Error::InvalidConfiguration);
    }
    let safe: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_catalog.pg_trigger t \
         WHERE t.tgrelid='public.advisory_budget_policies'::regclass \
         AND t.tgname='advisory_budget_policy_guard_trigger' AND t.tgenabled='A' AND NOT t.tgisinternal)"
    ).fetch_one(pool).await.map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    validate_catalog_structure(pool).await?;
    Ok(())
}

async fn validate_catalog_structure(pool: &PgPool) -> Result<()> {
    // Structural declarations only; deployed expression equality and RLS behavior
    // require a separately authorized canonical catalog read and runtime proof.
    let safe: bool = sqlx::query_scalar(
        "WITH expected(relation,name,function,type,enabled) AS (VALUES \
         ('advisory_dispatch','advisory_dispatch_matrix_choice_guard','advisory_dispatch_require_matrix_choice',7,'O'), \
         ('advisory_budget_policies','advisory_budget_policy_guard_trigger','advisory_budget_policy_guard',31,'A'), \
         ('advisory_budget_reservations','advisory_budget_reservation_guard_trigger','advisory_budget_reservation_guard',31,'O'), \
         ('advisory_budget_consumptions','advisory_budget_consumption_guard_trigger','advisory_budget_consumption_guard',31,'O'), \
         ('advisory_budget_consumptions','z_advisory_budget_consumption_exhaustion_guard','advisory_budget_consumption_exhaustion_guard',7,'O'), \
         ('advisory_budget_reservations','zz_advisory_budget_global_reservation_guard','advisory_budget_global_reservation_guard',7,'O'), \
         ('advisory_budget_consumptions','zz_advisory_budget_global_consumption_guard','advisory_budget_global_consumption_guard',7,'O'), \
         ('advisory_provider_observations','advisory_provider_observation_guard_trigger','advisory_provider_observation_guard',31,'O'), \
         ('matrix_v1_dispatch_cutover_allowlist','matrix_v1_cutover_allowlist_immutable','matrix_v1_cutover_allowlist_immutable',31,'O'), \
         ('advisory_matrix_advice','matrix_v1_cutover_advice_insert_guard','matrix_v1_cutover_advice_insert_guard',7,'O')) \
         SELECT count(*)=10 AND COALESCE(bool_and(COALESCE( \
         NOT t.tgisinternal AND t.tgtype=e.type AND t.tgenabled::text=e.enabled \
         AND t.tgnargs=0 AND pg_catalog.octet_length(t.tgargs)=0 \
         AND pn.nspname='public' AND p.proname=e.function AND p.pronargs=0 \
         AND p.prorettype='pg_catalog.trigger'::regtype,false)),false) \
         FROM expected e \
         LEFT JOIN pg_catalog.pg_namespace n ON n.nspname='public' \
         LEFT JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=e.relation \
         LEFT JOIN pg_catalog.pg_trigger t ON t.tgrelid=c.oid AND t.tgname=e.name \
         LEFT JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid \
         LEFT JOIN pg_catalog.pg_namespace pn ON pn.oid=p.pronamespace"
    ).fetch_one(pool).await.map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    let query = format!(
        "WITH expected(relation,name,command,checked) AS (VALUES \
         ('advisory_matrix_advice','advisory_matrix_advice_tenant_scope','*',true), \
         ('advisory_provider_observations','advisory_provider_observations_tenant_scope','*',true), \
         ('advisory_budget_policies','advisory_budget_policy_tenant_scope','*',true), \
         ('advisory_budget_reservations','advisory_budget_reservation_tenant_scope','*',true), \
         ('advisory_budget_consumptions','advisory_budget_consumption_tenant_scope','*',true), \
         ('matrix_v1_dispatch_cutover_allowlist','matrix_v1_cutover_allowlist_tenant_read','r',false)) \
         SELECT count(*)=6 AND COALESCE(bool_and(COALESCE( \
         p.polcmd::text=e.command AND p.polpermissive AND p.polroles=ARRAY[0::oid] \
         AND p.polqual IS NOT NULL AND (p.polwithcheck IS NOT NULL)=e.checked \
         AND (NOT e.checked OR pg_catalog.pg_get_expr(p.polqual,p.polrelid) \
              =pg_catalog.pg_get_expr(p.polwithcheck,p.polrelid)),false)),false) \
         AND (SELECT count(*) FROM pg_catalog.pg_policy allp \
              JOIN pg_catalog.pg_class allc ON allc.oid=allp.polrelid \
              JOIN pg_catalog.pg_namespace alln ON alln.oid=allc.relnamespace \
              WHERE alln.nspname='public' AND allc.relname IN ({TABLES}))=6 \
         FROM expected e \
         LEFT JOIN pg_catalog.pg_namespace n ON n.nspname='public' \
         LEFT JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=e.relation \
         LEFT JOIN pg_catalog.pg_policy p ON p.polrelid=c.oid AND p.polname=e.name"
    );
    let safe: bool = sqlx::query_scalar(&query)
        .fetch_one(pool)
        .await
        .map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

pub(super) async fn grant_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
    runtime_role: &str,
) -> Result<()> {
    let statements = [
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE advisory_matrix_advice,advisory_provider_observations, \
            advisory_budget_policies,advisory_budget_reservations,advisory_budget_consumptions, \
            matrix_v1_dispatch_cutover_allowlist FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE advisory_matrix_advice,advisory_provider_observations, \
            advisory_budget_reservations,advisory_budget_consumptions TO {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT ON TABLE advisory_budget_policies TO {quoted_role}"),
        format!("GRANT UPDATE(id) ON TABLE advisory_budget_policies TO {quoted_role}"),
        format!("GRANT SELECT ON TABLE matrix_v1_dispatch_cutover_allowlist TO {quoted_role}"),
    ];
    for statement in statements {
        sqlx::query(&statement)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    let query = format!(
        "SELECT count(*)::bigint,COALESCE(bool_and(c.relrowsecurity AND c.relforcerowsecurity \
         AND pg_catalog.has_table_privilege($1,c.oid,'SELECT') \
         AND pg_catalog.has_table_privilege($1,c.oid,'INSERT')=(c.relname<>'matrix_v1_dispatch_cutover_allowlist') \
         AND NOT pg_catalog.has_table_privilege($1,c.oid,'UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') \
         AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) acl \
                        WHERE acl.grantee=0)),false) \
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relkind='r' AND c.relname IN ({TABLES})"
    );
    let (count, safe): (i64, bool) = sqlx::query_as(&query)
        .bind(runtime_role)
        .fetch_one(&mut **transaction)
        .await
        .map_err(storage_error)?;
    if count != 6 || !safe {
        return Err(Error::InvalidConfiguration);
    }
    let query = format!(
        "SELECT COALESCE(bool_and(pg_catalog.has_column_privilege($1,c.oid,a.attnum,'UPDATE') \
         = (c.relname='advisory_budget_policies' AND a.attname='id')),false) \
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid \
         WHERE n.nspname='public' AND c.relname IN ({TABLES}) \
         AND a.attnum>0 AND NOT a.attisdropped"
    );
    let safe: bool = sqlx::query_scalar(&query)
        .bind(runtime_role)
        .fetch_one(&mut **transaction)
        .await
        .map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
