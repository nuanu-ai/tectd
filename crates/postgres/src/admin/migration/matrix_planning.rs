use super::*;

#[cfg(test)]
#[path = "matrix_planning/bootstrap_tests.rs"]
mod bootstrap_tests;

const TABLES: &str = "'matrix_planning_selection_links','matrix_planning_effect_attestations'";
const FUNCTIONS: &str = "'matrix_planning_selection_require_active_owner', \
    'matrix_planning_selection_require_mapped_nodes','matrix_planning_effect_require_active_verifier', \
    'matrix_planning_lock_context','matrix_planning_selection_require_context', \
    'matrix_planning_effect_require_context','matrix_verification_bindings_require_unconsumed_v2', \
    'matrix_planning_lock_verification'";

pub(super) async fn grant_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
    runtime_role: &str,
) -> Result<()> {
    for statement in [
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE matrix_planning_selection_links, \
                 matrix_planning_effect_attestations FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE matrix_planning_selection_links, \
                 matrix_planning_effect_attestations TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION \
                 public.matrix_planning_selection_require_active_owner(), \
                 public.matrix_planning_selection_require_mapped_nodes(), \
                 public.matrix_planning_effect_require_active_verifier(), \
                 public.matrix_planning_lock_context(uuid,uuid,uuid,bigint,text,text,text,uuid,text,text), \
                 public.matrix_planning_selection_require_context(), \
                 public.matrix_planning_effect_require_context(), \
                 public.matrix_verification_bindings_require_unconsumed_v2() FROM {quoted_role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION \
                 public.matrix_planning_lock_verification(uuid,uuid,uuid) TO {quoted_role}"
        ),
    ] {
        sqlx::query(&statement)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    validate_runtime_role_connection(transaction, runtime_role, true).await
}

pub(super) async fn validate_runtime_role(pool: &PgPool, runtime_role: &str) -> Result<()> {
    let mut connection = pool.acquire().await.map_err(storage_error)?;
    validate_runtime_role_connection(&mut connection, runtime_role, true).await
}

pub(super) async fn validate_runtime_role_pregrant(
    pool: &PgPool,
    runtime_role: &str,
) -> Result<()> {
    let mut connection = pool.acquire().await.map_err(storage_error)?;
    validate_runtime_role_connection(&mut connection, runtime_role, false).await
}

async fn validate_runtime_role_connection(
    connection: &mut sqlx::PgConnection,
    runtime_role: &str,
    require_grants: bool,
) -> Result<()> {
    // Pregrant and postgrant privilege validation share all catalog/security checks.
    // Only required positive grants are deferred during bootstrap.
    let query = format!(
        "SELECT count(*)=2 AND COALESCE(bool_and(c.relrowsecurity AND c.relforcerowsecurity \
         AND NOT pg_catalog.pg_has_role($1,c.relowner,'MEMBER') \
         AND (NOT $2 OR (pg_catalog.has_table_privilege($1,c.oid,'SELECT') \
             AND pg_catalog.has_table_privilege($1,c.oid,'INSERT'))) \
         AND NOT pg_catalog.has_table_privilege($1,c.oid,'UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER') \
         AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a \
             WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped \
               AND pg_catalog.has_column_privilege($1,c.oid,a.attnum,'UPDATE')) \
         AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode( \
             COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) acl \
             WHERE acl.grantee=0)),false) \
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relkind='r' AND c.relname IN ({TABLES})"
    );
    let safe: bool = sqlx::query_scalar(&query)
        .bind(runtime_role)
        .bind(require_grants)
        .fetch_one(&mut *connection)
        .await
        .map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    let query = format!(
        "SELECT count(*)=8 AND COALESCE(bool_and(p.prosecdef AND p.provolatile='v' \
         AND NOT pg_catalog.pg_has_role($1,p.proowner,'MEMBER') \
         AND (NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE') OR \
             p.oid='public.matrix_planning_lock_verification(uuid,uuid,uuid)'::regprocedure) \
         AND (NOT $2 OR pg_catalog.has_function_privilege($1,p.oid,'EXECUTE')= \
             (p.oid='public.matrix_planning_lock_verification(uuid,uuid,uuid)'::regprocedure)) \
         AND NOT EXISTS(SELECT 1 FROM pg_catalog.aclexplode( \
             COALESCE(p.proacl,pg_catalog.acldefault('f',p.proowner))) acl \
             WHERE acl.grantee=0 AND acl.privilege_type='EXECUTE') \
         AND EXISTS(SELECT 1 FROM pg_catalog.unnest(p.proconfig) cfg \
             WHERE pg_catalog.replace(cfg,' ','')='search_path=pg_catalog,public,pg_temp') \
         AND CASE WHEN p.proname='matrix_planning_lock_context' THEN \
             p.oid='public.matrix_planning_lock_context(uuid,uuid,uuid,bigint,text,text,text,uuid,text,text)'::regprocedure \
             AND p.pronargs=10 AND p.prorettype='pg_catalog.void'::regtype \
         WHEN p.proname='matrix_planning_lock_verification' THEN \
             p.oid='public.matrix_planning_lock_verification(uuid,uuid,uuid)'::regprocedure \
             AND p.pronargs=3 AND p.prorettype='pg_catalog.void'::regtype \
         ELSE p.pronargs=0 AND p.prorettype='pg_catalog.trigger'::regtype END),false) \
         FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
         WHERE n.nspname='public' AND p.proname IN ({FUNCTIONS})"
    );
    let safe: bool = sqlx::query_scalar(&query)
        .bind(runtime_role)
        .bind(require_grants)
        .fetch_one(&mut *connection)
        .await
        .map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    validate_catalog_structure(connection).await
}

async fn validate_catalog_structure(connection: &mut sqlx::PgConnection) -> Result<()> {
    let safe: bool = sqlx::query_scalar(
        "WITH expected(relation,name,function,type) AS (VALUES \
         ('matrix_planning_selection_links','matrix_planning_selection_active_owner','matrix_planning_selection_require_active_owner',7), \
         ('matrix_planning_selection_links','matrix_planning_selection_immutable','matrix_verification_deny_mutation',27), \
         ('matrix_planning_selection_links','matrix_planning_selection_mapped_nodes_guard','matrix_planning_selection_require_mapped_nodes',7), \
         ('matrix_planning_selection_links','matrix_planning_selection_context_guard','matrix_planning_selection_require_context',7), \
         ('matrix_planning_effect_attestations','matrix_planning_effect_active_verifier','matrix_planning_effect_require_active_verifier',7), \
         ('matrix_planning_effect_attestations','matrix_planning_effect_immutable','matrix_verification_deny_mutation',27), \
         ('matrix_planning_effect_attestations','matrix_planning_effect_context_guard','matrix_planning_effect_require_context',7), \
         ('matrix_verification_bindings','matrix_verification_bindings_unconsumed_v2_guard','matrix_verification_bindings_require_unconsumed_v2',7)) \
         SELECT count(*)=8 AND COALESCE(bool_and(COALESCE( \
         NOT t.tgisinternal AND t.tgtype=e.type AND t.tgenabled='O' \
         AND t.tgnargs=0 AND pg_catalog.octet_length(t.tgargs)=0 \
         AND pn.nspname='public' AND p.proname=e.function AND p.pronargs=0 \
         AND p.prorettype='pg_catalog.trigger'::regtype,false)),false) \
         FROM expected e \
         LEFT JOIN pg_catalog.pg_namespace n ON n.nspname='public' \
         LEFT JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=e.relation \
         LEFT JOIN pg_catalog.pg_trigger t ON t.tgrelid=c.oid AND t.tgname=e.name \
         LEFT JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid \
         LEFT JOIN pg_catalog.pg_namespace pn ON pn.oid=p.pronamespace"
    ).fetch_one(&mut *connection).await.map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    let query = format!(
        "WITH expected(relation,name) AS (VALUES \
         ('matrix_planning_selection_links','matrix_planning_selection_links_tenant_scope'), \
         ('matrix_planning_effect_attestations','matrix_planning_effect_attestations_tenant_scope')) \
         SELECT count(*)=2 AND COALESCE(bool_and(COALESCE( \
         p.polcmd='*' AND p.polpermissive AND p.polroles=ARRAY[0::oid] \
         AND p.polqual IS NOT NULL AND p.polwithcheck IS NOT NULL \
         AND pg_catalog.pg_get_expr(p.polqual,p.polrelid)=pg_catalog.pg_get_expr(p.polwithcheck,p.polrelid),false)),false) \
         AND (SELECT count(*) FROM pg_catalog.pg_policy allp \
              JOIN pg_catalog.pg_class allc ON allc.oid=allp.polrelid \
              JOIN pg_catalog.pg_namespace alln ON alln.oid=allc.relnamespace \
              WHERE alln.nspname='public' AND allc.relname IN ({TABLES}))=2 \
         FROM expected e \
         LEFT JOIN pg_catalog.pg_namespace n ON n.nspname='public' \
         LEFT JOIN pg_catalog.pg_class c ON c.relnamespace=n.oid AND c.relname=e.relation \
         LEFT JOIN pg_catalog.pg_policy p ON p.polrelid=c.oid AND p.polname=e.name"
    );
    let safe: bool = sqlx::query_scalar(&query)
        .fetch_one(&mut *connection)
        .await
        .map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
