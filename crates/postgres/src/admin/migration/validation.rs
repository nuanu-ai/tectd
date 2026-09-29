use super::*;

pub async fn validate_runtime_role(pool: &PgPool, runtime_role: &str) -> Result<()> {
    let role: Option<(bool, bool, bool)> = sqlx::query_as(
        r#"
        SELECT r.rolsuper,
               r.rolbypassrls,
               EXISTS (
                   SELECT 1 FROM pg_catalog.pg_database d
                   WHERE d.datname=pg_catalog.current_database() AND pg_catalog.pg_has_role(r.oid, d.datdba, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1 FROM pg_catalog.pg_namespace n
                   WHERE n.nspname='public' AND pg_catalog.pg_has_role(r.oid, n.nspowner, 'MEMBER')
               ) OR EXISTS (
                   SELECT 1
                   FROM pg_catalog.pg_class c
                   JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace
                   WHERE n.nspname='public'
                     AND c.relname IN (
                         'tenants', 'principals', 'hosts', 'workspaces', 'memberships',
                         'agent_sessions', 'session_advisory_preference_history', 'source_repositories', 'source_worktrees',
                         'session_worktrees', 'workspace_events', 'programs', 'program_inputs',
                         'advisory_workspace_config', 'advisory_workspace_config_history',
                         'advisory_opportunity', 'advisory_dispatch',
                         'advisory_matrix_advice', 'advisory_matrix_disposition',
                         'matrix_planning_selection_links',
                         'matrix_planning_effect_attestations',
                         'pipeline_open_effect_attestations',
                         'pipeline_phase_effect_attestations',
                         'scope_anti_bloat_preservation_attestations',
                         'pipeline_advice_contexts',
                         'matrix_tasks', 'matrix_task_revisions', 'matrix_task_requirements_bindings',
                         'matrix_verifications', 'matrix_verification_bindings',
                         'advisory_scope_source_snapshot', 'advisory_scope_manifest',
                         'advisory_scope_advice', 'advisory_scope_disposition',
                         'advisory_scope_preservation_receipt',
                         'advisory_scope_caller_link', 'advisory_scope_verifier_receipt',
                         'advisory_scope_selected_save_observation',
                         'setup_session_directories', 'workspace_setups', 'workspace_setup_inputs',
                         'scope_candidate_sets', 'scope_candidate_inputs',
                         'scope_candidate_contents', 'scope_candidate_snapshots',
                         'scope_candidate_source_refs', 'scope_candidate_drafts',
                         'scope_candidate_reviews', 'scope_candidate_receipts', 'scope_candidate_delta_receipts', 'scope_candidate_delta_operations',
                         'scope_candidate_delta_candidates', 'scope_candidate_delta_goals', 'scope_candidate_delta_coverage',
                         'scope_candidate_delta_evidence', 'scope_candidate_delta_blockers', 'scope_candidate_delta_supersessions',
                         'native_scopes', 'slice_candidate_sets', 'slice_planning_inputs',
                         'slice_planning_snapshots', 'slice_candidate_drafts',
                         'slice_candidate_reviews', 'native_slices', 'slice_results',
                         'native_planning_receipts', 'slice_pipeline_runs',
                         'slice_pipeline_phase_attempts', 'slice_pipeline_phase_outputs',
                         'slice_pipeline_output_bindings', 'slice_pipeline_inputs',
                         'slice_pipeline_receipts','slice_pipeline_run_migrations','pipeline_delivery_receipts','pipeline_evidence_artifacts',
                         'durable_knowledge_capability','workspace_knowledge_state','knowledge_changes','knowledge_unit_heads',
                         'knowledge_publication_events','knowledge_revisions','knowledge_bindings',
                         'knowledge_command_receipts','pipeline_knowledge_manifests','pipeline_knowledge_manifest_resources','knowledge_effect_outbox',
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
                   JOIN pg_catalog.pg_namespace n ON n.oid=p.pronamespace
                   WHERE n.nspname='public'
                     AND p.proname IN ('tect_authenticate_host', 'tect_preserve_created_at',
                         'tect_dk_native_publish','tect_dk_native_read','tect_dk_native_owned_residual','tect_dk_session_principal','tect_dk_is_owner','tect_dk_ensure_workspace_state','tect_dk_capability','tect_dk_database_identity_ready',
                         'tect_dk_internal_native_publish','tect_dk_internal_native_read','tect_dk_internal_native_owned_residual','tect_dk2_internal_native_publish','tect_dk2_internal_native_read','tect_dk_internal_native_erase','tect_dk_internal_capability','tect_dk_search_vector_ready')
                     AND pg_catalog.pg_has_role(r.oid, p.proowner, 'MEMBER')
               )
        FROM pg_catalog.pg_roles r
        WHERE r.rolname=$1
        "#,
    )
    .bind(runtime_role)
    .fetch_optional(pool)
    .await
    .map_err(storage_error)?;
    let (superuser, bypass_rls, owns_database_object) = role.ok_or(Error::InvalidConfiguration)?;
    if superuser || bypass_rls || owns_database_object {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    const GRANTS: &str = include_str!("../migration.rs");
    const VALIDATION: &str = include_str!("validation.rs");

    #[test]
    fn paged_manifest_runtime_grants_are_scoped_to_capture_read_and_erasure() {
        assert!(GRANTS.contains(
            "REVOKE ALL PRIVILEGES ON TABLE pipeline_knowledge_manifest_resources FROM {quoted_role}"
        ));
        assert!(GRANTS.contains(
            "GRANT SELECT,INSERT,DELETE ON TABLE pipeline_knowledge_manifest_resources TO {quoted_role}"
        ));
        assert!(
            !GRANTS.contains(
                &[
                    "GRANT UPDATE",
                    " ON TABLE pipeline_knowledge_manifest_resources"
                ]
                .concat()
            )
        );
        assert!(
            !GRANTS.contains(
                &[
                    "GRANT ALL",
                    " ON TABLE pipeline_knowledge_manifest_resources"
                ]
                .concat()
            )
        );
        assert!(GRANTS.contains(
            "resource_count,total_resource_bytes,resource_digest_algorithm,payload_erased) ON TABLE pipeline_knowledge_manifests"
        ));
        assert!(VALIDATION.contains(
            "'pipeline_knowledge_manifests','pipeline_knowledge_manifest_resources','knowledge_effect_outbox'"
        ));
    }
}
