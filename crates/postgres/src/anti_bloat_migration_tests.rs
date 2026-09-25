const MIGRATION: &str = include_str!("../migrations/0076_scope_anti_bloat_review.sql");
const NATIVE_APPLY: &str = include_str!("../migrations/0078_scope_anti_bloat_native_apply.sql");

#[test]
fn binding_foreign_key_targets_exact_source_identity() {
    assert!(
        MIGRATION
            .contains("ADD CONSTRAINT advisory_scope_manifest_anti_bloat_source_unique UNIQUE")
    );
    assert!(
        MIGRATION
            .contains("(tenant_id,workspace_id,opportunity_id,candidate_set_id,source_digest)")
    );
    assert!(MIGRATION.contains("CONSTRAINT scope_anti_bloat_binding_manifest_fk FOREIGN KEY"));
}

#[test]
fn dispatch_and_response_are_one_use_audit_transitions() {
    assert!(MIGRATION.contains("OLD.state='prepared' AND NEW.state='sending'"));
    assert!(MIGRATION.contains("OLD.request_bytes IS NULL AND NEW.request_bytes IS NOT NULL"));
    assert!(MIGRATION.contains("OLD.state='sending' AND NEW.state='sending'"));
    assert!(MIGRATION.contains("OLD.raw_response IS NULL AND NEW.raw_response IS NOT NULL"));
    assert!(
        MIGRATION.contains("OLD.raw_response IS NOT NULL AND NEW.raw_response=OLD.raw_response")
    );
    assert!(
        MIGRATION
            .contains("state <> 'ranked' OR (raw_response IS NOT NULL AND ranked_ids IS NOT NULL)")
    );
    assert!(MIGRATION.contains("OLD.state='sending' AND NEW.state='send_unknown'"));
}

#[test]
fn source_binding_and_caller_receipt_cannot_be_rewritten() {
    assert!(MIGRATION.contains("BEFORE UPDATE OR DELETE ON scope_anti_bloat_bindings"));
    assert!(MIGRATION.contains("BEFORE UPDATE OR DELETE ON scope_anti_bloat_caller_links"));
    assert!(MIGRATION.contains("after_material_digest text NOT NULL"));
    assert!(MIGRATION.contains("after_payload jsonb NOT NULL"));
    assert!(MIGRATION.contains("caller_receipt jsonb NOT NULL"));
}

#[test]
fn native_apply_receipt_links_both_immutable_draft_revisions() {
    assert!(NATIVE_APPLY.contains("'anti_bloat_narrow'"));
    assert!(NATIVE_APPLY.contains("scope_anti_bloat_native_caller_receipt_fk"));
    assert!(NATIVE_APPLY.contains("scope_anti_bloat_native_before_draft_fk"));
    assert!(NATIVE_APPLY.contains("scope_anti_bloat_native_after_draft_fk"));
    assert!(NATIVE_APPLY.contains("scope_anti_bloat_native_idempotency_unique"));
    assert!(NATIVE_APPLY.contains("DROP CONSTRAINT scope_anti_bloat_caller_delta_fk"));
}
