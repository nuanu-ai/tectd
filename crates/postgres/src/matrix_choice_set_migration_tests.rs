const MIGRATION: &str = include_str!("../migrations/0048_matrix_task_choice_set.sql");
const RUNTIME_GRANTS: &str = include_str!("admin/migration.rs");

#[test]
fn choice_set_is_an_optional_versioned_part_of_the_exact_revision() {
    for required in [
        "ALTER TABLE matrix_task_revisions",
        "ADD COLUMN choice_set_schema text",
        "ADD COLUMN choice_set jsonb",
        "ADD COLUMN choice_set_digest text",
        "choice_set_schema IS NULL AND choice_set IS NULL AND choice_set_digest IS NULL",
        "choice_set_schema IS NOT NULL",
        "choice_set_schema = 'tect.matrix-choice-set/1'",
        "choice_set IS NOT NULL",
        "pg_catalog.jsonb_typeof(choice_set) = 'object'",
        "choice_set_digest IS NOT NULL",
        "choice_set_digest ~ '^[0-9a-f]{64}$'",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(!MIGRATION.contains("NOT NULL DEFAULT"));
    assert!(!MIGRATION.contains("UPDATE matrix_task_revisions"));
    assert!(!MIGRATION.contains("matrix_tasks"));
    assert!(!MIGRATION.contains("programs"));
}

#[test]
fn runtime_cannot_rewrite_accepted_choice_sets() {
    assert!(RUNTIME_GRANTS.contains(
        "REVOKE ALL PRIVILEGES ON TABLE matrix_tasks, matrix_task_revisions FROM {quoted_role}"
    ));
    assert!(RUNTIME_GRANTS.contains(
        "GRANT SELECT, INSERT ON TABLE matrix_tasks, matrix_task_revisions TO {quoted_role}"
    ));
    assert!(!RUNTIME_GRANTS.contains("GRANT UPDATE ON TABLE matrix_task_revisions"));
    assert!(!RUNTIME_GRANTS.contains("GRANT UPDATE(choice_set"));
}
