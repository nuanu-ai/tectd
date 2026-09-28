const MIGRATION: &str = include_str!("../migrations/0111_session_advisory_preference.sql");

#[test]
fn session_preference_has_stored_default_cas_revision_and_append_only_history() {
    for fragment in [
        "advisory_preference text NOT NULL DEFAULT 'use_workspace'",
        "advisory_preference_revision bigint NOT NULL DEFAULT 0",
        "PRIMARY KEY (tenant_id, workspace_id, session_id, revision)",
        "REFERENCES public.agent_sessions (tenant_id, workspace_id, id)",
        "ENABLE ROW LEVEL SECURITY",
        "FORCE ROW LEVEL SECURITY",
    ] {
        assert!(MIGRATION.contains(fragment), "missing {fragment}");
    }
}
