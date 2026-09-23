use super::*;

const SCOPE_TABLES: [&str; 7] = [
    "advisory_scope_source_snapshot",
    "advisory_scope_manifest",
    "advisory_scope_advice",
    "advisory_scope_disposition",
    "advisory_scope_preservation_receipt",
    "advisory_scope_caller_link",
    "advisory_scope_verifier_receipt",
];

pub(super) async fn grant_scope_advisory_runtime(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    quoted_role: &str,
) -> Result<()> {
    let statements = [
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE {} FROM {quoted_role}",
            SCOPE_TABLES.join(",")
        ),
        format!(
            "GRANT SELECT ON TABLE {} TO {quoted_role}",
            SCOPE_TABLES.join(",")
        ),
        format!(
            "GRANT INSERT ON TABLE {} TO {quoted_role}",
            SCOPE_TABLES.join(",")
        ),
    ];
    for statement in statements {
        sqlx::query(&statement)
            .execute(&mut **transaction)
            .await
            .map_err(storage_error)?;
    }
    Ok(())
}

pub(super) async fn validate_advisory_schema(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    runtime_role: &str,
) -> Result<()> {
    let schema_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) = 4 AND pg_catalog.bool_and(c.relrowsecurity AND c.relforcerowsecurity) \
         FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relkind='r' AND c.relname=ANY($1)",
    )
    .bind([
        "advisory_workspace_config",
        "advisory_workspace_config_history",
        "advisory_opportunity",
        "advisory_dispatch",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let indexes_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) = 4 FROM pg_catalog.pg_indexes \
         WHERE schemaname='public' AND indexname=ANY($1)",
    )
    .bind([
        "advisory_opportunity_workspace_audit_idx",
        "advisory_opportunity_scope_audit_idx",
        "advisory_dispatch_unresolved_idx",
        "advisory_dispatch_retry_child_unique",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let policies_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) = 4 FROM pg_catalog.pg_policies \
         WHERE schemaname='public' AND policyname=ANY($1)",
    )
    .bind([
        "advisory_workspace_config_tenant_scope",
        "advisory_workspace_config_history_tenant_scope",
        "advisory_opportunity_tenant_scope",
        "advisory_dispatch_tenant_scope",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let constraints_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) = 11 FROM pg_catalog.pg_constraint \
         WHERE connamespace='public'::regnamespace AND conname=ANY($1)",
    )
    .bind([
        "advisory_workspace_config_model_check",
        "advisory_workspace_config_history_model_check",
        "advisory_workspace_config_history_predecessor_fk",
        "advisory_workspace_config_history_fk",
        "advisory_opportunity_decision_capability_check",
        "advisory_opportunity_state_reason_check",
        "advisory_dispatch_opportunity_fk",
        "advisory_dispatch_predecessor_fk",
        "advisory_dispatch_attempt_unique",
        "advisory_dispatch_lifecycle_check",
        "advisory_dispatch_retry_basis_check",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let public_revoked: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS( \
             SELECT 1 FROM pg_catalog.pg_class c \
             CROSS JOIN LATERAL pg_catalog.aclexplode( \
                 COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner)) \
             ) acl \
             JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
             WHERE n.nspname='public' AND c.relname=ANY($1) AND acl.grantee=0 \
         )",
    )
    .bind([
        "advisory_workspace_config",
        "advisory_workspace_config_history",
        "advisory_opportunity",
        "advisory_dispatch",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let runtime_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.has_table_privilege($1,'public.advisory_workspace_config','SELECT') \
         AND pg_catalog.has_table_privilege($1,'public.advisory_workspace_config','INSERT') \
         AND pg_catalog.has_table_privilege($1,'public.advisory_workspace_config_history','SELECT') \
         AND pg_catalog.has_table_privilege($1,'public.advisory_workspace_config_history','INSERT') \
         AND pg_catalog.has_table_privilege($1,'public.advisory_opportunity','SELECT') \
         AND pg_catalog.has_table_privilege($1,'public.advisory_opportunity','INSERT') \
         AND pg_catalog.has_table_privilege($1,'public.advisory_dispatch','SELECT') \
         AND pg_catalog.has_table_privilege($1,'public.advisory_dispatch','INSERT') \
         AND pg_catalog.has_column_privilege($1,'public.advisory_workspace_config','revision','UPDATE') \
         AND pg_catalog.has_column_privilege($1,'public.advisory_opportunity','state','UPDATE') \
         AND pg_catalog.has_column_privilege($1,'public.advisory_dispatch','state','UPDATE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.advisory_workspace_config_history','UPDATE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.advisory_workspace_config','DELETE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.advisory_workspace_config_history','DELETE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.advisory_opportunity','DELETE') \
         AND NOT pg_catalog.has_table_privilege($1,'public.advisory_dispatch','DELETE')",
    )
    .bind(runtime_role)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let slice_schema_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=7 AND pg_catalog.bool_and(c.relrowsecurity AND c.relforcerowsecurity) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relkind='r' AND c.relname=ANY($1)",
    )
    .bind(SCOPE_TABLES)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let slice_public_revoked: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM pg_catalog.pg_class c CROSS JOIN LATERAL pg_catalog.aclexplode(COALESCE(c.relacl,pg_catalog.acldefault('r',c.relowner))) acl JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname=ANY($1) AND acl.grantee=0)",
    )
    .bind(SCOPE_TABLES)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let slice_constraints_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=19 FROM pg_catalog.pg_constraint \
         WHERE connamespace='public'::regnamespace AND conname=ANY($1)",
    )
    .bind([
        "advisory_scope_source_opportunity_fk",
        "advisory_scope_source_candidate_fk",
        "advisory_scope_source_snapshot_fk",
        "advisory_scope_manifest_source_fk",
        "advisory_scope_advice_manifest_fk",
        "advisory_scope_advice_dispatch_fk",
        "advisory_scope_disposition_advice_fk",
        "advisory_scope_disposition_predecessor_fk",
        "advisory_scope_disposition_actor_fk",
        "advisory_scope_disposition_session_fk",
        "advisory_scope_preservation_disposition_fk",
        "advisory_scope_preservation_manifest_fk",
        "advisory_scope_caller_preservation_fk",
        "advisory_scope_caller_receipt_fk",
        "advisory_scope_caller_actor_fk",
        "advisory_scope_caller_session_fk",
        "advisory_scope_verifier_caller_fk",
        "advisory_scope_verifier_actor_fk",
        "advisory_scope_verifier_session_fk",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let slice_checks_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=23 FROM pg_catalog.pg_constraint \
         WHERE connamespace='public'::regnamespace AND conname=ANY($1)",
    )
    .bind([
        "advisory_opportunity_preselection_target_check",
        "advisory_scope_source_revision_check",
        "advisory_scope_source_digest_check",
        "advisory_scope_source_schema_check",
        "advisory_scope_source_payload_check",
        "advisory_scope_manifest_digest_check",
        "advisory_scope_manifest_schema_check",
        "advisory_scope_manifest_payload_check",
        "advisory_scope_advice_digest_check",
        "advisory_scope_advice_schema_check",
        "advisory_scope_advice_payload_check",
        "advisory_scope_disposition_revision_check",
        "advisory_scope_disposition_action_check",
        "advisory_scope_disposition_schema_check",
        "advisory_scope_disposition_payload_check",
        "advisory_scope_preservation_revision_check",
        "advisory_scope_preservation_status_check",
        "advisory_scope_preservation_schema_check",
        "advisory_scope_preservation_payload_check",
        "advisory_scope_caller_passed_check",
        "advisory_scope_caller_revision_check",
        "advisory_scope_verifier_revision_check",
        "advisory_scope_verifier_digest_check",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let slice_unique_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=14 FROM pg_catalog.pg_constraint \
         WHERE connamespace='public'::regnamespace AND conname=ANY($1)",
    )
    .bind([
        "advisory_opportunity_candidate_material_unique",
        "advisory_scope_source_identity_unique",
        "advisory_scope_manifest_identity_unique",
        "advisory_scope_advice_opportunity_unique",
        "advisory_scope_advice_candidate_unique",
        "advisory_scope_disposition_request_unique",
        "advisory_scope_disposition_revision_unique",
        "advisory_scope_disposition_chain_unique",
        "advisory_scope_disposition_one_successor_unique",
        "advisory_scope_preservation_request_unique",
        "advisory_scope_preservation_candidate_unique",
        "advisory_scope_caller_request_unique",
        "advisory_scope_caller_candidate_unique",
        "advisory_scope_verifier_request_unique",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let slice_indexes_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=4 FROM pg_catalog.pg_indexes \
         WHERE schemaname='public' AND indexname=ANY($1)",
    )
    .bind([
        "advisory_scope_candidate_lookup_idx",
        "advisory_scope_disposition_lookup_idx",
        "advisory_scope_disposition_one_root_unique",
        "advisory_scope_caller_lookup_idx",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let slice_policies_ready: bool = sqlx::query_scalar(
        "SELECT pg_catalog.count(*)=7 AND pg_catalog.bool_and( \
                policyname=tablename||'_tenant_scope' AND permissive='PERMISSIVE' \
                AND roles='{public}'::name[] AND cmd='ALL' AND qual=with_check \
                AND pg_catalog.regexp_replace(qual,'[[:space:]]+','','g') \
                    =pg_catalog.regexp_replace(pg_catalog.format( \
                        '((CURRENT_USER = pg_get_userbyid(( SELECT pg_class.relowner FROM pg_class WHERE (pg_class.oid = (%L::regclass)::oid)))) OR (tenant_id = (NULLIF(current_setting(''tect.tenant_id''::text, true), ''''::text))::uuid))', \
                        tablename),'[[:space:]]+','','g')) \
         FROM pg_catalog.pg_policies WHERE schemaname='public' AND policyname=ANY($1)",
    )
    .bind([
        "advisory_scope_source_snapshot_tenant_scope",
        "advisory_scope_manifest_tenant_scope",
        "advisory_scope_advice_tenant_scope",
        "advisory_scope_disposition_tenant_scope",
        "advisory_scope_preservation_receipt_tenant_scope",
        "advisory_scope_caller_link_tenant_scope",
        "advisory_scope_verifier_receipt_tenant_scope",
    ])
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let slice_runtime_ready: bool = sqlx::query_scalar(
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
    .bind(SCOPE_TABLES)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if !schema_ready
        || !indexes_ready
        || !policies_ready
        || !constraints_ready
        || !public_revoked
        || !runtime_ready
        || !slice_schema_ready
        || !slice_public_revoked
        || !slice_constraints_ready
        || !slice_checks_ready
        || !slice_unique_ready
        || !slice_indexes_ready
        || !slice_policies_ready
        || !slice_runtime_ready
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}
