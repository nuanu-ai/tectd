const MIGRATION: &str = include_str!("../migrations/0047_matrix_task_revisions.sql");
const RUNTIME_GRANTS: &str = include_str!("admin/migration.rs");

#[test]
fn task_head_and_accepted_revisions_have_tenant_bound_lineage() {
    for required in [
        "CREATE TABLE matrix_tasks (",
        "CREATE TABLE matrix_task_revisions (",
        "PRIMARY KEY (tenant_id, workspace_id, id)",
        "PRIMARY KEY (tenant_id, workspace_id, task_id, revision)",
        "REFERENCES workspaces (tenant_id, id)",
        "REFERENCES matrix_tasks (tenant_id, workspace_id, id)",
        "REFERENCES matrix_task_revisions (tenant_id, workspace_id, task_id, revision)",
        "REFERENCES principals (tenant_id, id)",
        "REFERENCES agent_sessions (tenant_id, workspace_id, id)",
        "DEFERRABLE INITIALLY DEFERRED",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert_eq!(MIGRATION.matches("CREATE TABLE matrix_").count(), 2);
}

#[test]
fn accepted_input_is_versioned_idempotent_and_keeps_fact_state() {
    for required in [
        "current_revision bigint NOT NULL",
        "previous_revision bigint",
        "request_id uuid NOT NULL",
        "UNIQUE (tenant_id, workspace_id, request_id)",
        "(revision = 1 AND previous_revision IS NULL)",
        "(revision > 1 AND previous_revision = revision - 1)",
        "input_schema = 'tect.engineering-matrix-input/1'",
        "canonical_input jsonb NOT NULL",
        "pg_catalog.jsonb_typeof(canonical_input) = 'object'",
        "input_digest ~ '^[0-9a-f]{64}$'",
        "recorded_by_principal_id uuid NOT NULL",
        "recorded_by_session_id uuid NOT NULL",
        "recorded_at timestamptz NOT NULL",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    for field in [
        "'mode'",
        "'envelope'",
        "'criticality'",
        "'intent'",
        "'urgency'",
        "'promised_behavior'",
        "'promised_proof'",
        "'affected_guarantees'",
        "'actual_exposure'",
        "'demand_commitment'",
        "'latency_commitment'",
        "'urgent_repair'",
    ] {
        assert!(MIGRATION.contains(field), "missing {field}");
    }
    for forbidden in [
        "verified_at",
        "verification_status",
        "CREATE TRIGGER",
        "advisory_dispatch",
    ] {
        assert!(!MIGRATION.contains(forbidden), "unexpected {forbidden}");
    }
}

#[test]
fn rls_and_runtime_grants_allow_only_append_and_head_cas() {
    for required in [
        "ENABLE ROW LEVEL SECURITY",
        "FORCE ROW LEVEL SECURITY",
        "tect.tenant_id",
        "REVOKE ALL PRIVILEGES ON TABLE matrix_tasks, matrix_task_revisions FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    for required in [
        "REVOKE ALL PRIVILEGES ON TABLE matrix_tasks, matrix_task_revisions FROM {quoted_role}",
        "GRANT SELECT, INSERT ON TABLE matrix_tasks, matrix_task_revisions TO {quoted_role}",
        "GRANT UPDATE(current_revision) ON TABLE matrix_tasks TO {quoted_role}",
        "'matrix_tasks', 'matrix_task_revisions'",
    ] {
        assert!(RUNTIME_GRANTS.contains(required), "missing {required}");
    }
    assert!(!RUNTIME_GRANTS.contains("GRANT UPDATE ON TABLE matrix_task_revisions"));
    assert!(!RUNTIME_GRANTS.contains("GRANT DELETE ON TABLE matrix_task_revisions"));
}
