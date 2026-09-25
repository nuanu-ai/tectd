const MIGRATION: &str = include_str!("../migrations/0058_matrix_disposition_active_owner.sql");
const STORE: &str = include_str!("matrix_disposition_store/implementation.rs");
const GRANTS: &str = include_str!("admin/migration.rs");
const SCHEMA_VALIDATOR: &str = include_str!("admin/matrix_advisory.rs");

#[test]
fn disposition_insert_locks_exact_active_owner_and_membership() {
    for required in [
        "CREATE FUNCTION matrix_disposition_require_active_owner() RETURNS trigger",
        "LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public",
        "NEW.tenant_id IS DISTINCT FROM",
        "pg_catalog.current_setting('tect.tenant_id', true)",
        "FROM public.agent_sessions AS s",
        "JOIN public.hosts AS h",
        "JOIN public.principals AS p",
        "JOIN public.memberships AS m",
        "s.tenant_id = NEW.tenant_id",
        "s.workspace_id = NEW.workspace_id",
        "s.id = NEW.session_id",
        "h.principal_id = NEW.actor_id",
        "m.principal_id = p.id",
        "AND NOT s.revoked AND NOT h.revoked AND p.role = 'owner'",
        "FOR SHARE OF s, h, p, m",
        "BEFORE INSERT ON advisory_matrix_disposition",
        "REVOKE ALL PRIVILEGES ON FUNCTION matrix_disposition_require_active_owner() FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
}

#[test]
fn runtime_uses_existing_definer_preflight_without_private_identity_grants() {
    assert!(STORE.contains("COALESCE(public.tect_dk_session_principal($1)=$2,false)"));
    assert!(STORE.contains("public.tect_dk_is_owner($2)"));
    assert!(!STORE.contains("FOR SHARE OF s,h,p,m"));
    assert!(GRANTS.contains("REVOKE ALL PRIVILEGES ON TABLE tenants, principals, hosts"));
    assert!(!MIGRATION.contains("GRANT SELECT ON TABLE hosts"));
    assert!(!MIGRATION.contains("GRANT SELECT ON TABLE principals"));
    assert!(SCHEMA_VALIDATOR.contains("advisory_matrix_disposition_active_owner"));
    assert!(SCHEMA_VALIDATOR.contains("p.prosecdef"));
    assert!(SCHEMA_VALIDATOR.contains("NOT pg_catalog.has_function_privilege($1,p.oid,'EXECUTE')"));
}
