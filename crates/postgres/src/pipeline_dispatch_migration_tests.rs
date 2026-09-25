const MIGRATION: &str = include_str!("../migrations/0065_pipeline_advice_dispatch_guard.sql");
const RUNTIME_GRANTS: &str = include_str!("admin/migration.rs");

#[test]
fn pipeline_dispatch_has_one_start_and_exact_request_binding() {
    for guard in [
        "opportunity.capability <> 'pipeline_recommendation'",
        "opportunity.state <> 'prepared'",
        "opportunity.primary_reason <> 'recommendation_prepared'",
        "opportunity.session_preference <> 'use_workspace'",
        "opportunity.request_preference <> 'use_workspace'",
        "NEW.attempt_number <> 1",
        "NEW.predecessor_dispatch_id IS NOT NULL",
        "EXISTS (SELECT 1 FROM public.advisory_dispatch AS prior",
        "NEW.material_digest <> opportunity.material_digest",
        "context.manifest_digest=NEW.material_digest",
        "pg_catalog.sha256(NEW.request_payload)",
        "OLD.state <> 'authorized' OR NEW.state <> 'sending'",
        "OLD.state='sending' AND NEW.state='sealed'",
        "pg_catalog.sha256(NEW.response_payload)",
    ] {
        assert!(MIGRATION.contains(guard), "missing dispatch guard: {guard}");
    }
}

#[test]
fn pipeline_start_rechecks_current_planning_and_matrix_path() {
    for guard in [
        "candidate.status='ready'",
        "review.payload->>'verdict'='ready'",
        "draft.set_revision<candidate.revision",
        "effect.verdict='match'",
        "disposition.outcome='selected'",
        "task.current_revision=selection.task_revision",
        "NOT session.revoked AND NOT host.revoked",
        "actor.id=opportunity.authorized_actor_id AND actor.role='owner'",
        "cfg.mode='optional'",
        "cfg.revision=opportunity.config_revision",
        "FOR SHARE OF context,candidate,snapshot,review,draft,effect,selection,disposition,task,receipt,session,host,actor,membership",
    ] {
        assert!(
            MIGRATION.contains(guard),
            "missing currentness guard: {guard}"
        );
    }
}

#[test]
fn runtime_can_write_exact_seal_digest() {
    assert!(RUNTIME_GRANTS.contains("GRANT UPDATE(response_payload,pipeline_response_sha256,"));
}
