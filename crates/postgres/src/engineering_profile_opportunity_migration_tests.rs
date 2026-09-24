const MIGRATION: &str = include_str!("../migrations/0049_engineering_profile_opportunity.sql");
const PREVIOUS: &str = include_str!("../migrations/0048_matrix_task_choice_set.sql");
const RUNTIME_GRANTS: &str = include_str!("admin/migration.rs");

#[test]
fn opportunity_decision_pair_is_widened_only_for_engineering_profile() {
    for required in [
        "DROP CONSTRAINT advisory_opportunity_decision_check",
        "ADD CONSTRAINT advisory_opportunity_decision_check CHECK",
        "DROP CONSTRAINT advisory_opportunity_decision_capability_check",
        "ADD CONSTRAINT advisory_opportunity_decision_capability_check CHECK",
        "capability = 'scope_decomposition'",
        "decision_point = 'scope.decomposition.before_selection'",
        "capability = 'engineering_profile'",
        "decision_point = 'engineering.profile.before_selection'",
        "pg_catalog.btrim(policy_version) <> ''",
        "pg_catalog.btrim(request_key) <> ''",
        "config_revision >= 0",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(!MIGRATION.contains("DROP CONSTRAINT advisory_opportunity_capability_check"));
    assert!(!MIGRATION.contains("DROP CONSTRAINT advisory_opportunity_preselection_target_check"));
}

#[test]
fn engineering_opportunity_has_exact_tenant_bound_choice_bearing_revision() {
    assert!(PREVIOUS.contains("ADD COLUMN choice_set_digest text"));
    for required in [
        "UNIQUE (tenant_id, workspace_id, task_id, revision, choice_set_digest)",
        "ADD COLUMN matrix_task_revision bigint",
        "ADD COLUMN matrix_choice_set_digest text",
        "work_item_kind = 'matrix_task'",
        "work_item_id IS NOT NULL",
        "scope_id IS NULL",
        "matrix_task_revision IS NOT NULL",
        "matrix_task_revision >= 1",
        "source_revision IS NOT NULL",
        "source_revision = matrix_task_revision::text",
        "matrix_choice_set_digest IS NOT NULL",
        "matrix_choice_set_digest ~ '^[0-9a-f]{64}$'",
        "matrix_task_revision IS NULL",
        "matrix_choice_set_digest IS NULL",
        "FOREIGN KEY (tenant_id, workspace_id, work_item_id,",
        "matrix_task_revision, matrix_choice_set_digest)",
        "REFERENCES matrix_task_revisions",
        "(tenant_id, workspace_id, task_id, revision, choice_set_digest)",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(!MIGRATION.contains("CREATE TABLE"));
    assert!(!MIGRATION.contains("advisory_scope_advice"));
}

#[test]
fn runtime_can_insert_opportunity_but_cannot_rebind_it() {
    assert!(RUNTIME_GRANTS.contains("advisory_opportunity, advisory_dispatch TO {quoted_role}"));
    assert!(RUNTIME_GRANTS.contains(
        "GRANT UPDATE(state,primary_reason,updated_at) ON TABLE advisory_opportunity TO {quoted_role}"
    ));
    assert!(!RUNTIME_GRANTS.contains("GRANT UPDATE(matrix_task_revision"));
    assert!(!RUNTIME_GRANTS.contains("GRANT UPDATE(matrix_choice_set_digest"));
}
