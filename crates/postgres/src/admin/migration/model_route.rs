use super::*;

const TABLES: [&str; 6] = [
    "model_route_preparations",
    "model_route_decisions",
    "model_route_dispositions",
    "model_route_advisory_attempts",
    "model_route_budget_reservations",
    "model_route_budget_consumptions",
];
const UPDATE_COLUMNS: [&str; 12] = [
    "state",
    "response_payload",
    "response_sha256",
    "raw_sealed_at",
    "parsed_outcome",
    "parsed_at",
    "response_http_status",
    "response_original_input_tokens",
    "response_original_output_tokens",
    "response_original_elapsed_ms",
    "response_complete",
    "original_transport_context",
];

pub(super) async fn grant_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
    runtime_role: &str,
) -> Result<()> {
    for table in TABLES {
        for statement in [
            format!("REVOKE ALL PRIVILEGES ON TABLE {table} FROM {quoted_role}"),
            format!("GRANT SELECT, INSERT ON TABLE {table} TO {quoted_role}"),
        ] {
            sqlx::query(&statement)
                .execute(&mut **transaction)
                .await
                .map_err(storage_error)?;
        }
    }
    sqlx::query(&format!(
        "GRANT UPDATE ({}) ON TABLE model_route_advisory_attempts TO {quoted_role}",
        UPDATE_COLUMNS.join(",")
    ))
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    sqlx::query(&format!(
        "REVOKE ALL PRIVILEGES ON TABLE advisory_call_audit FROM {quoted_role}"
    ))
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    sqlx::query(&format!(
        "GRANT SELECT ON TABLE advisory_call_audit TO {quoted_role}"
    ))
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    validate_connection(&mut **transaction, runtime_role, true).await
}

pub(super) async fn validate_runtime_role(pool: &PgPool, role: &str, grants: bool) -> Result<()> {
    let mut connection = pool.acquire().await.map_err(storage_error)?;
    validate_connection(&mut connection, role, grants).await
}

async fn validate_connection(
    connection: &mut sqlx::PgConnection,
    role: &str,
    grants: bool,
) -> Result<()> {
    // Effective grants, forced tenant RLS, immutable records, and sealed response
    // columns are checked independently of whether bootstrap has granted access.
    for table in TABLES {
        let safe: bool = sqlx::query_scalar(
            "SELECT count(*)=1 AND COALESCE(bool_and(c.relrowsecurity AND c.relforcerowsecurity \
             AND NOT pg_catalog.pg_has_role($1,c.relowner,'MEMBER') \
             AND NOT pg_catalog.has_table_privilege($1,c.oid,'UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') \
             AND (NOT $3 OR (pg_catalog.has_table_privilege($1,c.oid,'SELECT') AND pg_catalog.has_table_privilege($1,c.oid,'INSERT'))) \
             AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) a WHERE a.grantee=0) \
             AND (SELECT count(*)=1 AND COALESCE(bool_and(polcmd='*' AND polqual IS NOT NULL AND polwithcheck IS NOT NULL \
                 AND pg_catalog.pg_get_expr(polqual,polrelid)=pg_catalog.pg_get_expr(polwithcheck,polrelid)),false) \
                 FROM pg_catalog.pg_policy WHERE polrelid=c.oid) \
             AND EXISTS(SELECT 1 FROM pg_catalog.pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal AND t.tgenabled='O') \
             AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped \
                 AND pg_catalog.has_column_privilege($1,c.oid,a.attnum,'UPDATE') \
                 AND NOT ($2='model_route_advisory_attempts' AND a.attname=ANY($4)))) ,false) \
             FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname='public' AND c.relname=$2 AND c.relkind='r'"
        ).bind(role).bind(table).bind(grants).bind(UPDATE_COLUMNS.as_slice())
            .fetch_one(&mut *connection).await.map_err(storage_error)?;
        if !safe {
            return Err(Error::InvalidConfiguration);
        }
    }
    let columns_safe: bool = sqlx::query_scalar(
        "SELECT NOT $2 OR (SELECT count(*)=12 AND COALESCE(bool_and(pg_catalog.has_column_privilege($1,a.attrelid,a.attnum,'UPDATE')),false) \
         FROM pg_catalog.pg_attribute a WHERE a.attrelid='public.model_route_advisory_attempts'::regclass \
           AND a.attnum>0 AND NOT a.attisdropped AND a.attname=ANY($3))"
    ).bind(role).bind(grants).bind(UPDATE_COLUMNS.as_slice())
        .fetch_one(&mut *connection).await.map_err(storage_error)?;
    let view_safe: bool = sqlx::query_scalar(
        "SELECT count(*)=1 AND COALESCE(bool_and(c.relkind='v' \
         AND 'security_invoker=true'=ANY(c.reloptions) \
         AND NOT pg_catalog.pg_has_role($1,c.relowner,'MEMBER') \
         AND NOT pg_catalog.has_table_privilege($1,c.oid,'INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') \
         AND (NOT $2 OR pg_catalog.has_table_privilege($1,c.oid,'SELECT')) \
         AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode(COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) a WHERE a.grantee=0)),false) \
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname='advisory_call_audit'"
    ).bind(role).bind(grants).fetch_one(&mut *connection).await.map_err(storage_error)?;
    if !columns_safe || !view_safe {
        return Err(Error::InvalidConfiguration);
    }
    let safe: bool = sqlx::query_scalar(
        "SELECT count(*)=7 AND COALESCE(bool_and(NOT pg_catalog.pg_has_role($1,p.proowner,'MEMBER') \
         AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE') \
         AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode(COALESCE(p.proacl,pg_catalog.acldefault('f',p.proowner))) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE')),false) \
         FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
         WHERE n.nspname='public' AND p.pronargs=0 AND p.proname=ANY($2)"
    ).bind(role).bind(&["model_route_receipt_immutable", "model_route_attempt_guard", "model_route_budget_guard", "model_route_budget_visibility_guard", "model_route_native_observation_guard", "advisory_budget_global_reservation_guard", "advisory_budget_global_consumption_guard"])
        .fetch_one(&mut *connection).await.map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
