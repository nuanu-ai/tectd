const MIGRATION: &str = include_str!("../migrations/0059_matrix_planning_selection_link.sql");
const STORE: &str = include_str!("matrix_planning_selection_store.rs");
const GRANTS: &str = include_str!("admin/matrix_advisory.rs");

#[test]
fn link_is_immutable_and_bound_to_exact_real_save_receipt() {
    for required in [
        "PRIMARY KEY (tenant_id, workspace_id, candidate_set_id, caller_request_id)",
        "matrix_planning_selection_receipt_fk FOREIGN KEY",
        "(tenant_id, workspace_id, candidate_set_id, operation, caller_request_id)",
        "REFERENCES native_planning_receipts",
        "CHECK (operation = 'save_slice_draft')",
        "matrix_planning_selection_disposition_fk FOREIGN KEY",
        "REFERENCES advisory_matrix_disposition",
        "BEFORE UPDATE OR DELETE ON matrix_planning_selection_links",
        "ALTER TABLE matrix_planning_selection_links FORCE ROW LEVEL SECURITY",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    for required in [
        "request.get(\"matrix_selection\") != Some(&expected_selection)",
        "result.pointer(\"/candidate_set/revision\")",
        ".lock_matrix_task(workspace_id, link.selection.task_id)",
        ".matrix_verification_for_revision(",
        "matrix_verified_disposition_digest(&current.input, &composition, set, &validated)",
        "ON CONFLICT DO NOTHING RETURNING caller_request_id",
        "Some(prior) if prior == *link => Ok(())",
    ] {
        assert!(STORE.contains(required), "missing {required}");
    }
}

#[test]
fn caller_guard_locks_live_identity_without_private_runtime_grants() {
    for required in [
        "LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public",
        "s.id=NEW.caller_session_id",
        "h.principal_id=NEW.caller_principal_id",
        "AND NOT s.revoked AND NOT h.revoked AND p.role='owner'",
        "FOR SHARE OF s,h,p,m",
        "REVOKE ALL PRIVILEGES ON FUNCTION matrix_planning_selection_require_active_owner() FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(GRANTS.contains("matrix_planning_selection_links"));
    assert!(GRANTS.contains("matrix_planning_selection_active_owner"));
    assert!(!MIGRATION.contains("GRANT SELECT ON TABLE hosts"));
    assert!(!MIGRATION.contains("GRANT SELECT ON TABLE principals"));
}
