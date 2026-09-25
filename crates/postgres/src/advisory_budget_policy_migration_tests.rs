const MIGRATION: &str = include_str!("../migrations/0083_advisory_budget_policy.sql");
const GRANTS: &str = include_str!("admin/migration.rs");

#[test]
fn policy_is_explicit_owner_scoped_and_append_only() {
    for required in [
        "CREATE TABLE advisory_budget_policies",
        "provider_calls bigint NOT NULL CHECK (provider_calls > 0)",
        "input_tokens bigint NOT NULL CHECK (input_tokens > 0)",
        "output_tokens bigint NOT NULL CHECK (output_tokens > 0)",
        "request_utf8_bytes bigint NOT NULL CHECK (request_utf8_bytes > 0)",
        "elapsed_monotonic_ms bigint NOT NULL CHECK (elapsed_monotonic_ms > 0)",
        "retry_dispatches bigint NOT NULL CHECK (retry_dispatches > 0)",
        "UNIQUE (tenant_id, workspace_id, version)",
        "p.role='owner' AND m.workspace_id=NEW.workspace_id",
        "budget policy digest mismatch",
        "TG_OP <> 'INSERT'",
        "FORCE ROW LEVEL SECURITY",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(!MIGRATION.contains("INSERT INTO advisory_budget_policies"));
    assert!(GRANTS.contains("GRANT SELECT, INSERT ON TABLE advisory_budget_policies"));
}
