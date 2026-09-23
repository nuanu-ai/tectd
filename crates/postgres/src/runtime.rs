use crate::storage_error;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use tect_domain::{Error, Result};

pub(crate) async fn verify_runtime_role(pool: &PgPool) -> Result<()> {
    let (superuser, bypass_rls, owns_database_object, native_access): (bool, bool, bool, bool) = sqlx::query_as(
        r#"
        SELECT r.rolsuper,
               r.rolbypassrls,
               EXISTS (
                   SELECT 1 FROM pg_catalog.pg_database d
                   WHERE d.datname = pg_catalog.current_database() AND pg_catalog.pg_has_role(r.oid, d.datdba, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1 FROM pg_catalog.pg_namespace n
                   WHERE n.nspname = 'public' AND pg_catalog.pg_has_role(r.oid, n.nspowner, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1
                   FROM pg_catalog.pg_class c
                   JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
                   WHERE n.nspname = 'public'
                     AND c.relname IN (
                         'tenants', 'principals', 'hosts', 'workspaces', 'memberships',
                         'agent_sessions', 'source_repositories', 'source_worktrees',
                         'session_worktrees', 'workspace_events', 'programs', 'program_inputs',
                         'advisory_workspace_config', 'advisory_workspace_config_history',
                         'advisory_opportunity', 'advisory_dispatch',
                         'advisory_scope_source_snapshot', 'advisory_scope_manifest',
                         'advisory_scope_advice', 'advisory_scope_disposition',
                         'advisory_scope_preservation_receipt',
                         'advisory_scope_caller_link', 'advisory_scope_verifier_receipt',
                         'setup_session_directories', 'workspace_setups', 'workspace_setup_inputs',
                         'scope_candidate_sets', 'scope_candidate_inputs',
                         'scope_candidate_contents', 'scope_candidate_snapshots',
                         'scope_candidate_source_refs', 'scope_candidate_drafts',
                         'scope_candidate_reviews', 'scope_candidate_receipts',
                         'native_scopes', 'slice_candidate_sets', 'slice_planning_inputs',
                         'slice_planning_snapshots', 'slice_candidate_drafts',
                         'slice_candidate_reviews', 'native_slices', 'slice_results',
                         'native_planning_receipts', 'slice_pipeline_runs',
                         'slice_pipeline_phase_attempts', 'slice_pipeline_phase_outputs',
                         'slice_pipeline_output_bindings', 'slice_pipeline_inputs',
                         'slice_pipeline_receipts','pipeline_delivery_receipts','pipeline_evidence_artifacts'
                         ,'durable_knowledge_capability','workspace_knowledge_state','knowledge_changes','knowledge_unit_heads',
                         'knowledge_publication_events','knowledge_revisions','knowledge_bindings',
                         'knowledge_command_receipts','pipeline_knowledge_manifests','knowledge_effect_outbox',
                         'knowledge_lifecycle_changes','knowledge_change_runs','knowledge_change_operations',
                         'knowledge_change_outputs','knowledge_change_attempts','knowledge_change_output_bindings',
                         'knowledge_change_inputs','knowledge_lifecycle_command_receipts','knowledge_validation_events',
                         'knowledge_lifecycle_effects','knowledge_owned_copies','knowledge_suppression_ledger',
                         'knowledge_suppression_exports','knowledge_supersessions',
                         'knowledge_search_capability','knowledge_search_resources',
                         'knowledge_search_embedding_jobs','knowledge_search_vectors',
                         'knowledge_maintenance_signals','knowledge_maintenance_tasks',
                         'knowledge_maintenance_command_receipts','knowledge_maintenance_consumers',
                         'planning_knowledge_manifests','program_knowledge_refresh_receipts',
                         'planning_knowledge_consumptions'
                     )
                     AND pg_catalog.pg_has_role(r.oid, c.relowner, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1
                   FROM pg_catalog.pg_proc p
                   JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
                   WHERE n.nspname = 'public'
                     AND p.proname IN ('tect_authenticate_host', 'tect_preserve_created_at',
                         'tect_dk_native_publish','tect_dk_native_read','tect_dk_session_principal','tect_dk_is_owner','tect_dk_ensure_workspace_state','tect_dk_capability','tect_dk_database_identity_ready',
                         'tect_dk_internal_native_publish','tect_dk_internal_native_read','tect_dk_internal_native_owned_residual','tect_dk2_internal_native_publish','tect_dk2_internal_native_read','tect_dk_internal_native_erase','tect_dk_internal_capability','tect_dk_search_vector_ready')
                     AND pg_catalog.pg_has_role(r.oid, p.proowner, 'MEMBER')
               ),
               EXISTS (SELECT 1 FROM pg_catalog.pg_namespace n WHERE n.nspname='pgrdf' AND pg_catalog.has_schema_privilege(CURRENT_USER,n.oid,'USAGE'))
               OR EXISTS (SELECT 1 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='pgrdf' AND pg_catalog.has_function_privilege(CURRENT_USER,p.oid,'EXECUTE'))
               OR EXISTS (SELECT 1 FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='pgrdf' AND ((c.relkind='S' AND pg_catalog.has_sequence_privilege(CURRENT_USER,c.oid,'USAGE')) OR (c.relkind<>'S' AND pg_catalog.has_table_privilege(CURRENT_USER,c.oid,'SELECT'))))
        FROM pg_catalog.pg_roles r
        WHERE r.rolname = CURRENT_USER
        "#,
    )
    .fetch_one(pool)
    .await
    .map_err(storage_error)?;

    if superuser || bypass_rls || owns_database_object || native_access {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

pub(crate) fn credential_digest(credential: &str) -> String {
    hex_lower(&Sha256::digest(credential.as_bytes()))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}
