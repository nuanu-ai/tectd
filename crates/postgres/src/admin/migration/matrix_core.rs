use super::*;

const TABLES: &str = "'matrix_tasks', 'matrix_task_revisions', 'matrix_verifications', \
    'matrix_verification_bindings', 'matrix_requirements_proposals', \
    'matrix_requirements_confirmations', 'matrix_requirements_snapshots', \
    'matrix_task_requirements_bindings'";
const FUNCTIONS: &str = "'matrix_tasks_enforce_revision_step', \
    'matrix_task_revisions_require_active_owner', 'matrix_verifications_require_active_verifier', \
    'matrix_verification_deny_mutation', 'matrix_requirements_anchor_guard', \
    'matrix_task_requirements_binding_guard'";

pub(super) async fn validate_runtime_role(pool: &PgPool, runtime_role: &str) -> Result<()> {
    let query = format!(
        "SELECT count(*)::bigint, COALESCE(bool_and(c.relrowsecurity AND c.relforcerowsecurity \
         AND NOT pg_catalog.pg_has_role($1,c.relowner,'MEMBER')),false) \
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relkind='r' AND c.relname IN ({TABLES})"
    );
    let (count, safe): (i64, bool) = sqlx::query_as(&query)
        .bind(runtime_role)
        .fetch_one(pool)
        .await
        .map_err(storage_error)?;
    if count != 8 || !safe {
        return Err(Error::InvalidConfiguration);
    }
    let query = format!(
        "SELECT count(*)::bigint, COALESCE(bool_and( \
         NOT pg_catalog.pg_has_role($1,p.proowner,'MEMBER')),false) \
         FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace \
         WHERE n.nspname='public' AND p.pronargs=0 AND p.proname IN ({FUNCTIONS})"
    );
    let (count, safe): (i64, bool) = sqlx::query_as(&query)
        .bind(runtime_role)
        .fetch_one(pool)
        .await
        .map_err(storage_error)?;
    if count != 6 || !safe {
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
            "REVOKE ALL PRIVILEGES ON TABLE matrix_tasks, matrix_task_revisions FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE matrix_tasks, matrix_task_revisions TO {quoted_role}"
        ),
        format!("GRANT UPDATE(current_revision) ON TABLE matrix_tasks TO {quoted_role}"),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE matrix_verifications, matrix_verification_bindings FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE matrix_verifications, matrix_verification_bindings TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE matrix_requirements_proposals,matrix_requirements_confirmations,matrix_requirements_snapshots FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT,INSERT ON TABLE matrix_requirements_proposals,matrix_requirements_confirmations,matrix_requirements_snapshots TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE matrix_task_requirements_bindings FROM {quoted_role}"
        ),
        format!("GRANT SELECT,INSERT ON TABLE matrix_task_requirements_bindings TO {quoted_role}"),
    ];
    for statement in statements {
        sqlx::query(&statement)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    let query = format!(
        "SELECT count(*)::bigint, COALESCE(bool_and(c.relrowsecurity AND c.relforcerowsecurity \
         AND pg_catalog.has_table_privilege($1,c.oid,'SELECT') \
         AND pg_catalog.has_table_privilege($1,c.oid,'INSERT') \
         AND NOT pg_catalog.has_table_privilege($1,c.oid,'UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')),false) \
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relkind='r' AND c.relname IN ({TABLES})"
    );
    let (count, safe): (i64, bool) = sqlx::query_as(&query)
        .bind(runtime_role)
        .fetch_one(&mut **transaction)
        .await
        .map_err(storage_error)?;
    if count != 8 || !safe {
        return Err(Error::InvalidConfiguration);
    }
    let query = format!(
        "SELECT COALESCE(bool_and(pg_catalog.has_column_privilege($1,c.oid,a.attnum,'UPDATE') \
         = (c.relname='matrix_tasks' AND a.attname='current_revision')),false) \
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
