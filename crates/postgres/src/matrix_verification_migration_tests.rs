const MIGRATION: &str = include_str!("../migrations/0052_matrix_verification.sql");
const GRANTS: &str = include_str!("admin/migration/matrix_core.rs");
const STORE: &str = include_str!("matrix_verification_store.rs");

#[test]
fn verification_and_bindings_are_exactly_revision_bound_and_immutable() {
    for required in [
        "UNIQUE (tenant_id, workspace_id, task_id, revision, input_digest)",
        "CREATE TABLE matrix_verifications (",
        "CREATE TABLE matrix_verification_bindings (",
        "(tenant_id, workspace_id, task_id, task_revision, input_digest)",
        "(tenant_id, workspace_id, task_id, revision, input_digest)",
        "verification_reason = 'matrix_facts_verified'",
        "policy_version text NOT NULL",
        "record_digest text NOT NULL",
        "PRIMARY KEY (tenant_id, workspace_id, verification_id, fact_path)",
        "BEFORE UPDATE OR DELETE ON matrix_verifications",
        "BEFORE UPDATE OR DELETE ON matrix_verification_bindings",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
}

#[test]
fn verifier_identity_is_database_enforced_without_task_update_authority() {
    for required in [
        "NEW.verifier_session_id",
        "r.recorded_by_principal_id=NEW.owner_principal_id",
        "p.id=NEW.verifier_principal_id AND p.role='verifier'",
        "p.id<>NEW.owner_principal_id",
        "AND NOT s.revoked AND NOT h.revoked",
        "FOR SHARE OF t, r, s, h, p, m",
        "ENABLE ROW LEVEL SECURITY",
        "FORCE ROW LEVEL SECURITY",
        "REVOKE ALL PRIVILEGES ON TABLE matrix_verifications, matrix_verification_bindings FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(GRANTS.contains("GRANT SELECT, INSERT ON TABLE matrix_verifications, matrix_verification_bindings TO {quoted_role}"));
    assert!(!GRANTS.contains("GRANT UPDATE ON TABLE matrix_verifications"));
    assert!(!GRANTS.contains("GRANT DELETE ON TABLE matrix_verifications"));
    assert!(!GRANTS.contains("GRANT UPDATE(current_revision) ON TABLE matrix_task_revisions"));
}

#[test]
fn store_rechecks_current_head_and_reconstructs_exact_record() {
    for required in [
        "t.current_revision=$4 FOR UPDATE OF t NOWAIT",
        "evaluate_matrix_verification(",
        "if prior == *record",
        "ORDER BY v.verified_at DESC,v.id DESC LIMIT 1",
        "canonical_digest()",
    ] {
        assert!(STORE.contains(required), "missing {required}");
    }
}
