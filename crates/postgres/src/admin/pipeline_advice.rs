use super::*;

pub(super) async fn grant_pipeline_advice_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
) -> Result<()> {
    for statement in [
        format!("REVOKE ALL PRIVILEGES ON TABLE pipeline_advice_contexts FROM {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE pipeline_advice_contexts TO {quoted_role}"),
        format!("REVOKE ALL PRIVILEGES ON TABLE pipeline_advice_dispositions FROM {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE pipeline_advice_dispositions TO {quoted_role}"),
    ] {
        sqlx::query(&statement)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) async fn validate_pipeline_advice_schema(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runtime_role: &str,
) -> Result<()> {
    let shape_ready: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_class c \
         JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         JOIN pg_catalog.pg_roles r ON r.rolname=$1 \
         WHERE n.nspname='public' AND c.relname='pipeline_advice_contexts' \
           AND c.relkind='r' AND c.relrowsecurity AND c.relforcerowsecurity \
           AND NOT pg_catalog.pg_has_role(r.oid,c.relowner,'MEMBER')) \
         AND (SELECT pg_catalog.count(*)=18 FROM pg_catalog.pg_attribute a \
              WHERE a.attrelid='public.pipeline_advice_contexts'::regclass \
                AND a.attnum>0 AND NOT a.attisdropped \
                AND a.attname=ANY(ARRAY['tenant_id','workspace_id','opportunity_id', \
                  'candidate_set_id','candidate_set_revision','planning_snapshot_id', \
                  'source_snapshot_id','work_node_id', \
                  'work_node_revision','source_snapshot_digest','matrix_disposition_id', \
                  'match_effect_attestation_id','catalogue_revision','catalogue_digest', \
                  'eligible_kind_ids','verification_contract_digest', \
                  'manifest_payload','manifest_digest'])) \
         AND (SELECT pg_catalog.count(*)=5 FROM pg_catalog.pg_constraint con \
              WHERE con.conrelid='public.pipeline_advice_contexts'::regclass \
                AND con.contype='f' AND con.convalidated) \
         AND (SELECT pg_catalog.count(*)=2 FROM pg_catalog.pg_constraint con \
              WHERE con.conrelid='public.pipeline_advice_contexts'::regclass \
                AND con.conname IN ('pipeline_advice_context_manifest_digest_check', \
                  'pipeline_advice_context_manifest_shape_check') AND con.contype='c') \
         AND EXISTS (SELECT 1 FROM pg_catalog.pg_policy p \
              WHERE p.polrelid='public.pipeline_advice_contexts'::regclass \
                AND p.polname='pipeline_advice_contexts_tenant_scope' \
                AND p.polcmd='*' AND p.polqual IS NOT NULL \
                AND p.polwithcheck IS NOT NULL)",
    )
    .bind(runtime_role)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let guards_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=3 FROM pg_catalog.pg_trigger t \
         JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid \
         JOIN pg_catalog.pg_roles r ON r.rolname=$1 \
         WHERE t.tgrelid='public.pipeline_advice_contexts'::regclass \
           AND t.tgenabled='O' AND NOT t.tgisinternal \
           AND ((t.tgname='pipeline_advice_context_current' \
                 AND p.proname='pipeline_advice_context_require_current' \
                 AND p.prosecdef AND NOT pg_catalog.pg_has_role(r.oid,p.proowner,'MEMBER') \
                 AND pg_catalog.strpos(pg_catalog.regexp_replace( \
                   pg_catalog.pg_get_functiondef(p.oid),'[[:space:]]+','','g'), \
                   'a.result_revision=draft.set_revision')>0 \
                 AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE')) \
             OR (t.tgname='pipeline_advice_context_manifest_shape' \
                 AND p.proname='pipeline_advice_manifest_require_shape' \
                 AND p.prosecdef AND NOT pg_catalog.pg_has_role(r.oid,p.proowner,'MEMBER') \
                 AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE')) \
             OR (t.tgname='pipeline_advice_context_immutable' \
                 AND p.proname='matrix_verification_deny_mutation'))",
    )
    .bind(runtime_role)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let transition_guard_ready: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_trigger t \
         JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid \
         JOIN pg_catalog.pg_roles r ON r.rolname=$1 \
         WHERE t.tgrelid='public.advisory_opportunity'::regclass \
           AND t.tgname='advisory_opportunity_pipeline_empty_no_call' \
           AND t.tgenabled='O' AND NOT t.tgisinternal \
           AND p.proname='pipeline_advice_preserve_empty_no_call' \
           AND p.prosecdef AND NOT pg_catalog.pg_has_role(r.oid,p.proowner,'MEMBER') \
           AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE'))",
    )
    .bind(runtime_role)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let grants_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.has_table_privilege($1,'public.pipeline_advice_contexts','SELECT') \
         AND pg_catalog.has_table_privilege($1,'public.pipeline_advice_contexts','INSERT') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_contexts','UPDATE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_contexts','DELETE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_contexts','TRUNCATE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_contexts','REFERENCES') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_contexts','TRIGGER') \
         AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_class c \
           CROSS JOIN LATERAL pg_catalog.aclexplode( \
             COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) acl \
           WHERE c.oid='public.pipeline_advice_contexts'::regclass AND acl.grantee=0)",
    )
    .bind(runtime_role)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if !shape_ready || !guards_ready || !transition_guard_ready || !grants_ready {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

pub(super) async fn validate_pipeline_disposition_schema(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runtime_role: &str,
) -> Result<()> {
    let valid: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_class c \
           JOIN pg_catalog.pg_roles r ON r.rolname=$1 \
           WHERE c.oid='public.pipeline_advice_dispositions'::regclass \
             AND c.relrowsecurity AND c.relforcerowsecurity \
             AND NOT pg_catalog.pg_has_role(r.oid,c.relowner,'MEMBER')) \
         AND (SELECT pg_catalog.count(*)=3 FROM pg_catalog.pg_trigger t \
              JOIN pg_catalog.pg_proc p ON p.oid=t.tgfoid \
              WHERE t.tgrelid='public.pipeline_advice_dispositions'::regclass \
                AND t.tgenabled='O' AND NOT t.tgisinternal \
                AND ((t.tgname='pipeline_advice_disposition_current' \
                    AND p.proname='pipeline_advice_disposition_guard' AND p.prosecdef \
                    AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE')) \
                  OR (t.tgname='pipeline_advice_disposition_response' \
                    AND p.proname='pipeline_advice_disposition_require_sealed_response' \
                    AND p.prosecdef \
                    AND NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE')) \
                  OR (t.tgname='pipeline_advice_disposition_immutable' \
                    AND p.proname='matrix_verification_deny_mutation'))) \
         AND pg_catalog.has_table_privilege($1,'public.pipeline_advice_dispositions','SELECT') \
         AND pg_catalog.has_table_privilege($1,'public.pipeline_advice_dispositions','INSERT') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_dispositions','UPDATE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_dispositions','DELETE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_dispositions','TRUNCATE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_dispositions','TRIGGER') \
         AND NOT EXISTS (SELECT 1 FROM pg_catalog.pg_class c \
           CROSS JOIN LATERAL pg_catalog.aclexplode( \
             COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) acl \
           WHERE c.oid='public.pipeline_advice_dispositions'::regclass AND acl.grantee=0)",
    )
    .bind(runtime_role)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if !valid {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
