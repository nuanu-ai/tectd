const MIGRATION: &str = include_str!("../migrations/0061_matrix_planning_effect_attestations.sql");
const ADMIN: &str = include_str!("admin/matrix_advisory.rs");
const ROLE: &str = include_str!("admin/migration.rs");

#[test]
fn attestation_binds_exact_link_and_receipt_and_is_append_only() {
    for required in [
        "PRIMARY KEY (tenant_id, workspace_id, id)",
        "(tenant_id, workspace_id, verifier_request_id)",
        "matrix_planning_effect_link_fk FOREIGN KEY",
        "REFERENCES matrix_planning_selection_links",
        "matrix_planning_effect_receipt_fk FOREIGN KEY",
        "REFERENCES native_planning_receipts",
        "CHECK (operation = 'save_slice_draft')",
        "CHECK (effect_digest ~ '^[0-9a-f]{64}$')",
        "verdict IN ('match', 'reject')",
        "pg_catalog.length(summary) <= 4096",
        "NEW.verified_at := pg_catalog.clock_timestamp()",
        "BEFORE UPDATE OR DELETE ON matrix_planning_effect_attestations",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
}

#[test]
fn verifier_guard_locks_live_save_and_independent_identity() {
    for required in [
        "LANGUAGE plpgsql SECURITY DEFINER SET search_path = pg_catalog, public",
        "c.revision=NEW.result_revision",
        "l.result_revision=NEW.result_revision",
        "AND NOT receipt.payload_erased",
        "receipt.request_payload IS NOT NULL",
        "receipt.result_payload IS NOT NULL",
        "pg_catalog.jsonb_array_length(l.mapped_nodes)>0",
        "p.id=NEW.verifier_principal_id AND p.role='verifier'",
        "p.id<>l.caller_principal_id",
        "p.id<>r.recorded_by_principal_id",
        "AND NOT s.revoked AND NOT h.revoked",
        "FOR SHARE OF l,c,receipt,r,s,h,p,m",
        "REVOKE ALL PRIVILEGES ON FUNCTION matrix_planning_effect_require_active_verifier() FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
}

#[test]
fn runtime_only_gets_select_and_insert_on_forced_rls_attestations() {
    for required in [
        "ALTER TABLE matrix_planning_effect_attestations FORCE ROW LEVEL SECURITY",
        "REVOKE ALL PRIVILEGES ON TABLE matrix_planning_effect_attestations FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(ADMIN.contains("matrix_planning_effect_attestations"));
    assert!(ADMIN.contains("GRANT SELECT, INSERT ON TABLE {tables}"));
    assert!(ADMIN.contains("AND NOT pg_catalog.has_table_privilege($1,pg_catalog.format('public.%I',table_name),'UPDATE')"));
    assert!(ADMIN.contains("matrix_planning_effect_active_verifier"));
    assert!(ROLE.contains("'matrix_planning_effect_attestations'"));
}
