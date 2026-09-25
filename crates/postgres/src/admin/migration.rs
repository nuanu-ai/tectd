use super::*;

pub async fn migrate(pool: &PgPool, runtime_role: &str) -> Result<()> {
    let quoted_role = quote_identifier(runtime_role)?;
    MIGRATOR.run(pool).await.map_err(storage_error)?;

    validate_runtime_role(pool, runtime_role).await?;

    let mut transaction = pool.begin().await.map_err(storage_error)?;
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
             pg_catalog.hashtextextended('tect-admin-runtime-grants', 0))",
    )
    .execute(&mut *transaction)
    .await
    .map_err(storage_error)?;
    let statements = [
        format!("GRANT USAGE ON SCHEMA public TO {quoted_role}"),
        format!("REVOKE ALL PRIVILEGES ON TABLE tenants, principals, hosts FROM {quoted_role}"),
        format!(
            "GRANT SELECT, INSERT ON TABLE workspaces, memberships, \
             agent_sessions, workspace_events TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE advisory_workspace_config, \
             advisory_workspace_config_history, advisory_opportunity, advisory_dispatch \
             FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE(revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id,updated_at) \
             ON TABLE advisory_workspace_config TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE advisory_workspace_config_history, \
             advisory_opportunity, advisory_dispatch TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(state,primary_reason,updated_at) ON TABLE advisory_opportunity TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(response_payload,pipeline_response_sha256,input_tokens,output_tokens,latency_ms,state,send_certainty,outcome,raw_response_ref,send_started_at,sealed_at) ON TABLE advisory_dispatch TO {quoted_role}"
        ),
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
            "GRANT SELECT, INSERT ON TABLE source_repositories, source_worktrees \
             TO {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT, DELETE ON TABLE session_worktrees TO {quoted_role}"),
        format!("REVOKE ALL PRIVILEGES ON TABLE programs, program_inputs FROM {quoted_role}"),
        format!("GRANT SELECT, INSERT, UPDATE ON TABLE programs TO {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE program_inputs TO {quoted_role}"),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE setup_session_directories, workspace_setups, \
             workspace_setup_inputs FROM {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT ON TABLE setup_session_directories TO {quoted_role}"),
        format!("GRANT SELECT, INSERT, UPDATE ON TABLE workspace_setups TO {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE workspace_setup_inputs TO {quoted_role}"),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE scope_candidate_sets, scope_candidate_inputs, \
             scope_candidate_contents, scope_candidate_snapshots, scope_candidate_source_refs, \
             scope_candidate_drafts, scope_candidate_reviews, scope_candidate_receipts, scope_candidate_delta_operations, \
             scope_candidate_delta_candidates, scope_candidate_delta_goals, scope_candidate_delta_coverage, \
             scope_candidate_delta_evidence, scope_candidate_delta_blockers, scope_candidate_delta_supersessions \
             FROM {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT, UPDATE ON TABLE scope_candidate_sets TO {quoted_role}"),
        format!(
            "GRANT SELECT, INSERT ON TABLE scope_candidate_inputs, scope_candidate_contents, \
             scope_candidate_snapshots, scope_candidate_source_refs, scope_candidate_drafts, \
             scope_candidate_reviews, scope_candidate_receipts, scope_candidate_delta_receipts, scope_candidate_delta_operations, \
             scope_candidate_delta_coverage, scope_candidate_delta_supersessions TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE ON TABLE scope_candidate_delta_candidates, \
             scope_candidate_delta_goals, scope_candidate_delta_evidence, scope_candidate_delta_blockers TO {quoted_role}"
        ),
        format!("GRANT DELETE ON TABLE scope_candidate_delta_coverage TO {quoted_role}"),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE scope_anti_bloat_bindings, \
             scope_anti_bloat_reviews, scope_anti_bloat_caller_links FROM {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT ON TABLE scope_anti_bloat_bindings TO {quoted_role}"),
        format!(
            "GRANT SELECT, INSERT ON TABLE scope_anti_bloat_reviews, \
             scope_anti_bloat_caller_links TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE scope_anti_bloat_preservation_attestations FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE scope_anti_bloat_preservation_attestations TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE model_route_preparations, \
             model_route_decisions, model_route_dispositions FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE model_route_preparations, \
             model_route_decisions, model_route_dispositions TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE model_route_advisory_attempts, advisory_call_audit FROM {quoted_role}"
        ),
        format!("GRANT SELECT, INSERT ON TABLE model_route_advisory_attempts TO {quoted_role}"),
        format!(
            "GRANT UPDATE(state,response_payload,response_sha256,raw_sealed_at,parsed_outcome,parsed_at) ON TABLE model_route_advisory_attempts TO {quoted_role}"
        ),
        format!("GRANT SELECT ON TABLE advisory_call_audit TO {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE advisory_budget_policies TO {quoted_role}"),
        format!("GRANT SELECT, INSERT ON TABLE advisory_budget_reservations TO {quoted_role}"),
        format!(
            "GRANT UPDATE(state,request_bytes,request_sha256,ranked_ids,raw_response,response_sha256,response_sealed_at,send_started_at,sealed_at) \
             ON TABLE scope_anti_bloat_reviews TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE native_scopes, slice_candidate_sets, \
             slice_planning_inputs, slice_planning_snapshots, slice_candidate_drafts, \
             slice_candidate_reviews, native_slices, slice_results, native_planning_receipts, \
             slice_pipeline_runs, slice_pipeline_phase_attempts, slice_pipeline_phase_outputs, \
             slice_pipeline_output_bindings, slice_pipeline_inputs, slice_pipeline_receipts, slice_pipeline_run_migrations, \
             pipeline_research_checkpoints, pipeline_checkpoint_receipts, pipeline_delivery_receipts, pipeline_evidence_artifacts \
             FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE ON TABLE native_scopes, slice_candidate_sets, \
             native_slices, slice_results TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE slice_planning_inputs, slice_planning_snapshots, \
             slice_candidate_drafts, slice_candidate_reviews, native_planning_receipts \
             TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE ON TABLE slice_pipeline_runs, \
             slice_pipeline_output_bindings TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE ON TABLE slice_pipeline_run_migrations TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT ON TABLE slice_pipeline_phase_attempts, \
             slice_pipeline_phase_outputs, slice_pipeline_inputs, \
             slice_pipeline_receipts, pipeline_checkpoint_receipts, pipeline_delivery_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE ON TABLE pipeline_evidence_artifacts TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT, INSERT, UPDATE ON TABLE pipeline_research_checkpoints TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE (result_payload) ON TABLE slice_pipeline_phase_attempts, \
             slice_pipeline_inputs TO {quoted_role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_authenticate_host(uuid, text, boolean) TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE workspace_knowledge_state,knowledge_changes,knowledge_unit_heads,knowledge_publication_events,knowledge_revisions,knowledge_bindings,knowledge_command_receipts,pipeline_knowledge_manifests,knowledge_effect_outbox FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT ON TABLE workspace_knowledge_state,knowledge_changes,knowledge_unit_heads,knowledge_publication_events,knowledge_revisions,knowledge_bindings,knowledge_command_receipts,pipeline_knowledge_manifests,knowledge_effect_outbox TO {quoted_role}"
        ),
        format!(
            "GRANT INSERT(tenant_id,workspace_id) ON TABLE workspace_knowledge_state TO {quoted_role}"
        ),
        format!("GRANT UPDATE(generation) ON TABLE workspace_knowledge_state TO {quoted_role}"),
        format!(
            "GRANT INSERT,UPDATE(stage,review,publication_receipt,updated_at) ON TABLE knowledge_changes TO {quoted_role}"
        ),
        format!(
            "GRANT INSERT,UPDATE(accepted_revision,active,proposal_fingerprint,last_event_id,updated_at) ON TABLE knowledge_unit_heads TO {quoted_role}"
        ),
        format!(
            "GRANT INSERT ON TABLE knowledge_publication_events,knowledge_revisions,knowledge_command_receipts,pipeline_knowledge_manifests,knowledge_effect_outbox TO {quoted_role}"
        ),
        format!("GRANT INSERT,UPDATE(active) ON TABLE knowledge_bindings TO {quoted_role}"),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk_session_principal(uuid),public.tect_dk_is_owner(uuid) TO {quoted_role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk_ensure_workspace_state(uuid,uuid) TO {quoted_role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk_capability(),public.tect_dk_database_identity_ready() TO {quoted_role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk_native_read(uuid,uuid,uuid,bigint,uuid) TO {quoted_role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk_native_owned_residual(uuid,uuid,uuid) TO {quoted_role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk_erased_no_change_proof_valid(jsonb) TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE knowledge_lifecycle_changes,knowledge_change_runs,knowledge_change_operations,knowledge_change_outputs,knowledge_change_attempts,knowledge_change_output_bindings,knowledge_change_inputs,knowledge_lifecycle_command_receipts,knowledge_validation_events,knowledge_lifecycle_effects,knowledge_owned_copies,knowledge_suppression_ledger,knowledge_suppression_exports,knowledge_supersessions FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT,INSERT,UPDATE ON TABLE knowledge_lifecycle_changes,knowledge_change_runs,knowledge_change_operations,knowledge_change_outputs,knowledge_change_output_bindings,knowledge_lifecycle_effects,knowledge_owned_copies,knowledge_suppression_ledger,knowledge_supersessions TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT,INSERT ON TABLE knowledge_change_attempts,knowledge_change_inputs,knowledge_lifecycle_command_receipts,knowledge_validation_events TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(output_digest,payload_erased) ON TABLE knowledge_change_attempts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(reason,input,digest,applied_basis_amendment,payload_erased) ON TABLE knowledge_change_inputs TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(erased_change_id,request_payload,result_payload,payload_erased) ON TABLE knowledge_lifecycle_command_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(sources,evidence_basis,source_pin_digest,payload_erased) ON TABLE knowledge_validation_events TO {quoted_role}"
        ),
        format!("GRANT SELECT ON TABLE knowledge_suppression_exports TO {quoted_role}"),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE knowledge_maintenance_signals,knowledge_maintenance_tasks,knowledge_maintenance_command_receipts,knowledge_maintenance_consumers FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT,INSERT ON TABLE knowledge_maintenance_signals,knowledge_maintenance_tasks,knowledge_maintenance_command_receipts,knowledge_maintenance_consumers TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(id,basis,basis_digest,payload_erased) ON TABLE knowledge_maintenance_signals TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(signal_id,revision,state,attempts,failure_code,next_retry_at,lease_token,lease_expires_at,change_id,run_id,current_review,affected_consumers,terminal_evidence,payload_erased,updated_at) ON TABLE knowledge_maintenance_tasks TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(request_payload,result_payload,payload_erased) ON TABLE knowledge_maintenance_command_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(active,required),DELETE ON TABLE knowledge_maintenance_consumers TO {quoted_role}"
        ),
        format!(
            "GRANT SELECT(erasure_sequence),UPDATE(erasure_sequence) ON TABLE durable_knowledge_capability TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(contract_version,lifecycle,access_scope,last_validation_event_id,payload_erased) ON TABLE knowledge_unit_heads TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(proposal_digest,proposal_fingerprint,source_sha256,semantic_diff,baseline,proposal,binding_provenance,reason,authority_basis,review,publication_receipt,payload_erased) ON TABLE knowledge_changes TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(constraint_payload,document_payload,payload_erased,source_sha256,rdf_digest) ON TABLE knowledge_revisions TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(event_payload,rdf_digest,payload_erased) ON TABLE knowledge_publication_events TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(request_payload,result_payload,payload_erased) ON TABLE knowledge_command_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(digest,semantic_digest,selected,unresolved_needs,definition_version,definition_digest,method_requirements,selected_resources,resource_unresolved_needs,freshness_warnings,resource_semantic_digest,resource_inquiry,resource_projection_policy,payload_erased) ON TABLE pipeline_knowledge_manifests TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(reviewer_context,request_payload,result_payload,payload_erased) ON TABLE slice_pipeline_phase_attempts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(body,producer_context_id,body_digest,reference,fields,verdict,dispositions,skill_reads,resource_reads,artifacts,evidence_artifacts,validator_receipts,followup_proposal,knowledge_publication,payload_erased) ON TABLE slice_pipeline_phase_outputs TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(input,input_digest,request_payload,result_payload,checkpoint_digest,payload_erased,owner_unit_ids) ON TABLE slice_pipeline_inputs TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(digest,producer_output_digest,basis,question,answer_criteria,inquiry,reason,consumer_terminal_output_digest,resolution_reason,payload_erased) ON TABLE pipeline_research_checkpoints TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(request_payload,result_payload,owner_unit_ids,payload_erased) ON TABLE pipeline_checkpoint_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(request_payload,result_payload,payload_erased,owner_unit_ids) ON TABLE slice_pipeline_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(input,payload_erased) ON TABLE slice_planning_inputs TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(result_ids,payload_erased) ON TABLE slice_planning_snapshots TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(payload,payload_erased,owner_unit_ids) ON TABLE slice_candidate_drafts,slice_candidate_reviews TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(request_payload,result_payload,payload_erased,owner_unit_ids) ON TABLE native_planning_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(payload,payload_erased,owner_unit_ids) ON TABLE scope_candidate_drafts,scope_candidate_reviews TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(request_payload,result_payload,payload_erased,owner_unit_ids) ON TABLE scope_candidate_receipts TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE planning_knowledge_manifests,program_knowledge_refresh_receipts,planning_knowledge_consumptions FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT,INSERT ON TABLE planning_knowledge_manifests,program_knowledge_refresh_receipts,planning_knowledge_consumptions TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(task_context_digest,task_context,needs,selected,unresolved_needs,digest,payload_erased) ON TABLE planning_knowledge_manifests TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(request_payload,manifest_id,result_payload,payload_erased) ON TABLE program_knowledge_refresh_receipts TO {quoted_role}"
        ),
        format!("GRANT UPDATE(redacted) ON TABLE planning_knowledge_consumptions TO {quoted_role}"),
        format!(
            "REVOKE ALL PRIVILEGES ON TABLE planning_knowledge_manifests,program_knowledge_refresh_receipts,planning_knowledge_consumptions FROM {quoted_role}"
        ),
        format!(
            "GRANT SELECT,INSERT ON TABLE planning_knowledge_manifests,program_knowledge_refresh_receipts,planning_knowledge_consumptions TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(task_context_digest,task_context,needs,selected,unresolved_needs,digest,payload_erased) ON TABLE planning_knowledge_manifests TO {quoted_role}"
        ),
        format!(
            "GRANT UPDATE(request_payload,manifest_id,result_payload,payload_erased) ON TABLE program_knowledge_refresh_receipts TO {quoted_role}"
        ),
        format!("GRANT UPDATE(redacted) ON TABLE planning_knowledge_consumptions TO {quoted_role}"),
        format!(
            "GRANT UPDATE(request_payload,result_payload,payload_erased,owner_unit_ids) ON TABLE scope_candidate_receipts TO {quoted_role}"
        ),
        format!(
            "GRANT EXECUTE ON FUNCTION public.tect_dk2_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean),public.tect_dk_native_erase(uuid,uuid,uuid) TO {quoted_role}"
        ),
        format!(
            "REVOKE ALL PRIVILEGES ON FUNCTION public.tect_dk_internal_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk_internal_native_read(uuid,uuid,uuid,bigint,uuid),public.tect_dk_internal_native_owned_residual(uuid,uuid,uuid),public.tect_dk2_internal_native_publish(uuid,uuid,uuid,text,text,text),public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean),public.tect_dk_internal_native_erase(uuid,uuid,uuid),public.tect_dk_internal_capability() FROM {quoted_role}"
        ),
    ];
    for statement in statements {
        sqlx::query(&statement)
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
    }
    super::scope_advisory::grant_scope_advisory_runtime(&mut transaction, &quoted_role).await?;
    super::scope_advisory::validate_advisory_schema(&mut transaction, runtime_role).await?;
    super::matrix_advisory::grant_matrix_advisory_runtime(&mut transaction, &quoted_role).await?;
    super::matrix_advisory::validate_matrix_advisory_schema(&mut transaction, runtime_role).await?;
    super::pipeline_advice::grant_pipeline_advice_runtime(&mut transaction, &quoted_role).await?;
    super::pipeline_advice::validate_pipeline_advice_schema(&mut transaction, runtime_role).await?;
    super::pipeline_advice::validate_pipeline_disposition_schema(&mut transaction, runtime_role)
        .await?;
    super::pipeline_advice::validate_pipeline_open_effect_schema(&mut transaction, runtime_role)
        .await?;
    super::pipeline_advice::validate_pipeline_phase_effect_schema(&mut transaction, runtime_role)
        .await?;
    crate::knowledge_search_admin::grant_search_runtime(&mut transaction, runtime_role).await?;
    transaction.commit().await.map_err(storage_error)
}

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
                         'agent_sessions', 'source_repositories', 'source_worktrees',
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
                         'matrix_tasks', 'matrix_task_revisions',
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
