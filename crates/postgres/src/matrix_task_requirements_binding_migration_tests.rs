const SQL: &str = include_str!("../migrations/0054_matrix_task_requirements_binding.sql");

#[test]
fn matrix_task_requirements_binding_is_append_only_exact_and_new_revision_only() {
    for required in [
        "matrix_task_requirements_bindings",
        "matrix_requirements_snapshots",
        "TG_OP<>'INSERT'",
        "xmin",
        "request_id",
        "original_request_digest",
        "semantic_digest",
        "authority_schema",
        "FOR SHARE",
        "FORCE ROW LEVEL SECURITY",
        "REVOKE ALL PRIVILEGES",
    ] {
        assert!(SQL.contains(required), "missing {required}");
    }
    assert!(!SQL.contains("UPDATE public.matrix_task_requirements_bindings"));
    let grants = include_str!("admin/migration/matrix_core.rs");
    assert!(grants.contains("GRANT SELECT,INSERT ON TABLE matrix_task_requirements_bindings"));
    assert!(!grants.contains("GRANT UPDATE ON TABLE matrix_task_requirements_bindings"));
    assert!(!grants.contains("GRANT DELETE ON TABLE matrix_task_requirements_bindings"));
}
