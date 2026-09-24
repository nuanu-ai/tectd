const MIGRATION: &str = include_str!("../migrations/0062_pipeline_advice_contexts.sql");
const ADMIN: &str = include_str!("admin/pipeline_advice.rs");
const ROLE: &str = include_str!("admin/migration.rs");

#[test]
fn pipeline_decision_is_exact_and_pre_open() {
    for required in [
        "capability = 'pipeline_recommendation'",
        "decision_point = 'pipeline_recommendation_before_slice_open'",
        "work_item_kind = 'slice_candidate_node'",
        "run_id IS NULL AND phase IS NULL AND step IS NULL",
        "matrix_task_revision IS NULL AND matrix_choice_set_digest IS NULL",
        "matrix_verification_digest IS NULL",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(!MIGRATION.contains("GRANT UPDATE"));
    assert!(!MIGRATION.contains("GRANT DELETE"));
}

#[test]
fn context_is_frozen_to_current_selected_and_matched_work() {
    for required in [
        "PRIMARY KEY (tenant_id, workspace_id, opportunity_id)",
        "pipeline_advice_context_opportunity_fk FOREIGN KEY",
        "pipeline_advice_context_set_fk FOREIGN KEY",
        "pipeline_advice_context_snapshot_fk FOREIGN KEY",
        "pipeline_advice_context_disposition_fk FOREIGN KEY",
        "pipeline_advice_context_effect_fk FOREIGN KEY",
        "source_snapshot_digest ~ '^[0-9a-f]{64}$'",
        "catalogue_digest ~ '^[0-9a-f]{64}$'",
        "verification_contract_digest ~ '^[0-9a-f]{64}$'",
        "c.revision=NEW.candidate_set_revision",
        "c.current_snapshot_id=snapshot.id",
        "snapshot.source_snapshot_id=NEW.source_snapshot_id",
        "a.verdict='match'",
        "d.outcome='selected'",
        "l.disposition_id=NEW.matrix_disposition_id",
        "task.current_revision=l.task_revision",
        "node->>'node_id'=NEW.work_node_id::text",
        "node->>'node_revision'=NEW.work_node_revision::text",
        "NOT receipt.payload_erased",
        "FOR SHARE OF o,c,snapshot,a,l,d,task,receipt,s,h,p,m",
        "slice.custom-procedure-capture",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
}

#[test]
fn context_has_rls_immutable_guard_and_minimal_runtime_grants() {
    for required in [
        "BEFORE UPDATE OR DELETE ON pipeline_advice_contexts",
        "ALTER TABLE pipeline_advice_contexts FORCE ROW LEVEL SECURITY",
        "REVOKE ALL PRIVILEGES ON TABLE pipeline_advice_contexts FROM PUBLIC",
        "REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_context_require_current() FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(ADMIN.contains("GRANT SELECT, INSERT ON TABLE pipeline_advice_contexts"));
    assert!(ADMIN.contains(
        "NOT pg_catalog.has_table_privilege($1,'public.pipeline_advice_contexts','UPDATE')"
    ));
    assert!(ADMIN.contains("pipeline_advice_context_immutable"));
    assert!(ROLE.contains("'pipeline_advice_contexts'"));
    assert!(ROLE.contains("validate_pipeline_advice_schema"));
}
