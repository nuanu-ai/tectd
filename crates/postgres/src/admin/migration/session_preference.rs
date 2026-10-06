use super::*;

pub(super) async fn grant_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
    runtime_role: &str,
) -> Result<()> {
    sqlx::query(&format!(
        "GRANT UPDATE (advisory_preference, advisory_preference_revision) \
         ON TABLE agent_sessions TO {quoted_role}"
    ))
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    validate_connection(transaction, runtime_role, true).await
}

pub(super) async fn validate_runtime_role(
    pool: &PgPool,
    runtime_role: &str,
    require_grants: bool,
) -> Result<()> {
    let mut connection = pool.acquire().await.map_err(storage_error)?;
    validate_connection(&mut connection, runtime_role, require_grants).await
}

async fn validate_connection(
    connection: &mut sqlx::PgConnection,
    runtime_role: &str,
    require_grants: bool,
) -> Result<()> {
    // Effective privileges include grants inherited from roles and PUBLIC.
    // Only the two positive column grants are deferred before bootstrap.
    let safe: bool = sqlx::query_scalar(
        "SELECT count(*)=1 AND COALESCE(bool_and( \
         NOT pg_catalog.has_table_privilege($1,c.oid,'UPDATE') \
         AND NOT EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a \
             WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped \
               AND a.attname NOT IN ('advisory_preference','advisory_preference_revision') \
               AND pg_catalog.has_column_privilege($1,c.oid,a.attnum,'UPDATE')) \
         AND (NOT $2 OR (SELECT count(*)=2 AND COALESCE(bool_and( \
             pg_catalog.has_column_privilege($1,c.oid,a.attnum,'UPDATE')),false) \
             FROM pg_catalog.pg_attribute a WHERE a.attrelid=c.oid \
               AND a.attnum>0 AND NOT a.attisdropped \
               AND a.attname IN ('advisory_preference','advisory_preference_revision'))) \
         ),false) FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname='agent_sessions' AND c.relkind='r'",
    )
    .bind(runtime_role)
    .bind(require_grants)
    .fetch_one(&mut *connection)
    .await
    .map_err(storage_error)?;
    if !safe {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

#[cfg(test)]
pub(super) async fn assert_transaction_local_acl_cases(pool: &PgPool, role: &str) {
    async fn assert_original_acl(connection: &mut sqlx::PgConnection, role: &str) {
        let acl: (bool, bool, bool, bool) = sqlx::query_as(
            "SELECT pg_catalog.has_column_privilege($1,'public.agent_sessions', \
                 'advisory_preference','UPDATE'), \
             pg_catalog.has_column_privilege($1,'public.agent_sessions', \
                 'advisory_preference_revision','UPDATE'), \
             pg_catalog.has_table_privilege($1,'public.agent_sessions','UPDATE'), \
             EXISTS(SELECT 1 FROM pg_catalog.pg_attribute a \
                 WHERE a.attrelid='public.agent_sessions'::regclass \
                   AND a.attnum>0 AND NOT a.attisdropped \
                   AND a.attname NOT IN ('advisory_preference','advisory_preference_revision') \
                   AND pg_catalog.has_column_privilege($1,a.attrelid,a.attnum,'UPDATE'))",
        )
        .bind(role)
        .fetch_one(connection)
        .await
        .unwrap();
        assert_eq!(acl, (true, true, false, false));
    }

    let quoted = quote_identifier(role).unwrap();
    let mut connection = pool.acquire().await.unwrap();
    assert_original_acl(&mut connection, role).await;
    for (mutation, pregrant_allowed) in [
        (
            format!("REVOKE UPDATE (advisory_preference) ON agent_sessions FROM {quoted}"),
            true,
        ),
        (
            format!("REVOKE UPDATE (advisory_preference_revision) ON agent_sessions FROM {quoted}"),
            true,
        ),
        (format!("GRANT UPDATE ON agent_sessions TO {quoted}"), false),
        (
            format!("GRANT UPDATE (revoked) ON agent_sessions TO {quoted}"),
            false,
        ),
    ] {
        let mut transaction = sqlx::Connection::begin(&mut *connection).await.unwrap();
        sqlx::query(&mutation)
            .execute(&mut *transaction)
            .await
            .unwrap();
        let pregrant = validate_connection(&mut transaction, role, false).await;
        let strict = validate_connection(&mut transaction, role, true).await;
        transaction.rollback().await.unwrap();
        assert_eq!(pregrant.is_ok(), pregrant_allowed, "{mutation}");
        if !pregrant_allowed {
            assert!(
                matches!(pregrant, Err(Error::InvalidConfiguration)),
                "{mutation}"
            );
        }
        assert!(
            matches!(strict, Err(Error::InvalidConfiguration)),
            "{mutation}"
        );
        assert_original_acl(&mut connection, role).await;
    }
    assert_original_acl(&mut connection, role).await;
}
