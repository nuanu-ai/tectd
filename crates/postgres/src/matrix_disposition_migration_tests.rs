const MIGRATION: &str = include_str!("../migrations/0067_matrix_disposition.sql");
const STORE: &str = include_str!("matrix_disposition_store/implementation.rs");
const GRANTS: &str = include_str!("admin/migration.rs");
const SCHEMA_VALIDATOR: &str = include_str!("admin/migration/matrix_disposition.rs");

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

#[test]
fn disposition_migration_is_additive_not_duplicate_advice_or_planning_effect() {
    assert!(MIGRATION.contains("CREATE TABLE advisory_matrix_disposition"));
    assert!(!MIGRATION.contains("CREATE TABLE advisory_matrix_advice"));
    assert!(!MIGRATION.contains("matrix_planning_selection_links"));
    assert!(MIGRATION.contains("advisory_matrix_advice_disposition_binding_unique"));
    assert!(MIGRATION.contains("FORCE ROW LEVEL SECURITY"));
    assert!(
        MIGRATION
            .contains("REVOKE ALL PRIVILEGES ON TABLE advisory_matrix_disposition FROM PUBLIC")
    );
    assert!(MIGRATION.contains("advisory_matrix_disposition_request_unique"));
    assert!(MIGRATION.contains("advisory_matrix_disposition_opportunity_unique"));
}

#[test]
fn selected_and_blocked_after_advice_both_require_saved_current_verification() {
    assert!(STORE.contains("request.basis == MatrixDispositionBasis::AfterAdvice"));
    assert!(STORE.contains("evaluate_context_matrix_verification("));
    assert!(STORE.contains("matrix_verified_disposition_digest("));
    assert!(STORE.contains("source.requirements_binding.as_ref() != Some(binding)"));
    let advice = include_str!("matrix_disposition_store/implementation/advice_validation.rs");
    assert!(advice.contains("current_verification.ok_or(Error::StaleContext)?"));
    assert!(advice.contains("decode_trial_uncertainty("));
    assert!(advice.contains("trial != token.trial_evidence"));
    assert!(advice.contains("disposition_trial_snapshot_matches("));
}

#[test]
fn disposition_read_uses_trusted_bound_reader_without_widening_write_authority() {
    let app = include_str!("../../application/src/matrix_disposition.rs");
    assert!(app.contains("self.candidate_read_transaction(context).await?"));
    assert!(app.contains("PrincipalRole::Verifier => true"));
    assert!(app.contains("saved.recorded_by_principal_id == principal"));
    assert!(app.contains("saved.recorded_by_session_id == session"));
    assert!(app.contains("self.authorized(context, TransactionMode::ReadWrite)"));
    assert!(app.contains("tx.lock_native_session(identity.host_id, &context.native_session_id)"));
    assert!(STORE.contains("self.principal_id()? != actor_id"));
    assert!(STORE.contains("self.principal_role()?"));
    assert!(STORE.contains("public.tect_dk_is_owner($2)"));
    assert!(STORE.contains("ON CONFLICT DO NOTHING RETURNING disposition_id"));
}
