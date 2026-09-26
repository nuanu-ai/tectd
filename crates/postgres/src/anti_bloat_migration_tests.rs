const MIGRATION: &str = include_str!("../migrations/0076_scope_anti_bloat_review.sql");
const NATIVE_APPLY: &str = include_str!("../migrations/0078_scope_anti_bloat_native_apply.sql");
const VERIFIER: &str =
    include_str!("../migrations/0079_scope_anti_bloat_preservation_attestation.sql");
const ADAPTER: &str = include_str!("../migrations/0093_anti_bloat_adapter_identity.sql");
const TERMINALS: &str = include_str!("../migrations/0094_anti_bloat_terminal_outcomes.sql");
const OBSERVATION: &str = include_str!("../migrations/0095_anti_bloat_sealed_observation.sql");

#[test]
fn response_transport_metadata_preserves_historical_null_and_seal_binding() {
    let sql = include_str!("../migrations/0104_s04_s05_response_transport_metadata.sql");
    assert!(!sql.contains("DEFAULT"));
    assert!(!sql.contains("UPDATE public."));
    assert!(sql.contains("scope_anti_bloat_response_transport_requires_raw CHECK"));
    assert!(sql.contains("response_complete IS NULL AND original_transport_context IS NULL"));
    assert!(sql.contains("OLD.raw_response IS NOT NULL"));
    assert!(sql.contains("NEW.response_complete,NEW.original_transport_context) IS DISTINCT FROM"));
    assert!(sql.contains("OLD.response_complete,OLD.original_transport_context"));
    assert!(sql.contains(
        "OR NEW.response_complete IS NOT NULL OR NEW.original_transport_context IS NOT NULL"
    ));
    let seal = include_str!("anti_bloat_store/send.rs");
    let read = include_str!("anti_bloat_store/response.rs");
    assert!(seal.contains("response_complete=$12,original_transport_context=$13"));
    assert!(seal.contains("context.validate_for(&Some(observation.raw.clone()))?"));
    assert!(
        read.contains("response_original_elapsed_ms,response_complete,original_transport_context")
    );
    assert!(read.contains("response_complete: complete"));
    assert!(read.contains(".map(crate::advisory::decode_transport_context)"));
    assert!(
        include_str!("admin/migration.rs")
            .contains("response_complete,original_transport_context,send_started_at")
    );
}

#[test]
fn sealed_transport_metadata_is_nullable_immutable_and_runtime_writable() {
    for column in [
        "response_http_status",
        "response_original_input_tokens",
        "response_original_output_tokens",
        "response_original_elapsed_ms",
    ] {
        assert!(OBSERVATION.contains(column));
        assert!(include_str!("admin/migration.rs").contains(column));
    }
    assert!(!OBSERVATION.contains("DEFAULT"));
    assert!(!OBSERVATION.contains("UPDATE public.scope_anti_bloat_reviews"));
    assert!(OBSERVATION.contains("OLD.raw_response IS NOT NULL"));
    assert!(OBSERVATION.contains("SECURITY INVOKER SET search_path=pg_catalog,public,pg_temp"));
    assert!(OBSERVATION.contains("BEFORE UPDATE ON public.scope_anti_bloat_reviews"));
}

#[test]
fn adapter_identity_backfills_generic_and_freezes_with_exact_request() {
    assert!(ADAPTER.contains("NOT NULL DEFAULT 'generic-json-v1'"));
    assert!(ADAPTER.contains("CHECK (length(request_adapter_identity) > 0)"));
    assert!(ADAPTER.contains("OLD.request_bytes IS NOT NULL"));
    assert!(
        ADAPTER
            .contains("NEW.request_adapter_identity IS DISTINCT FROM OLD.request_adapter_identity")
    );
    assert!(ADAPTER.contains("BEFORE UPDATE ON scope_anti_bloat_reviews"));
}

#[test]
fn native_terminal_states_require_raw_seal_consumption_and_remain_immutable() {
    assert!(TERMINALS.contains("scope_anti_bloat_review_terminal_check CHECK"));
    assert!(TERMINALS.contains(
        "raw_response IS NOT NULL AND response_sealed_at IS NOT NULL AND sealed_at IS NOT NULL"
    ));
    assert!(TERMINALS.contains(
        "OLD.state='sending' AND NEW.state IN ('provider_abstained','invalid_response')"
    ));
    assert!(
        TERMINALS.contains(
            "c.request_sha256=OLD.request_sha256 AND c.response_sha256=OLD.response_sha256"
        )
    );
    assert!(TERMINALS.contains("NOT c.unknown_usage AND NOT c.exhausted_after_response"));
    assert!(TERMINALS.contains("NEW.request_adapter_identity) IS DISTINCT FROM"));
    assert!(TERMINALS.contains("NEW.raw_response,NEW.response_sha256,NEW.response_sealed_at"));
}

#[test]
fn guard_resolves_true_budget_table_and_preflight_cannot_freeze_a_send() {
    assert!(TERMINALS.contains("FUNCTION public.scope_anti_bloat_review_guard()"));
    assert!(TERMINALS.contains("SECURITY INVOKER SET search_path=pg_catalog,public,pg_temp"));
    assert!(TERMINALS.contains("FROM public.scope_anti_bloat_budget_consumptions c"));
    assert!(!TERMINALS.contains("FROM scope_anti_bloat_budget_consumptions c"));
    assert!(TERMINALS.contains("OLD.state='prepared' AND NEW.state IN ('provider_unconfigured'"));
    assert!(TERMINALS.contains("OLD.request_bytes IS NULL AND NEW.request_bytes IS NULL"));
    assert!(TERMINALS.contains("NEW.request_sha256 IS NULL AND NEW.send_started_at IS NULL"));
}

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

#[test]
fn verifier_attestation_is_independent_and_append_only() {
    assert!(VERIFIER.contains("scope_anti_bloat_preservation_attestation_guard"));
    assert!(VERIFIER.contains("vp.role='verifier'"));
    assert!(VERIFIER.contains("vp.id<>r.actor_id AND vp.id<>c.actor_id AND vp.id<>d.actor_id"));
    assert!(VERIFIER.contains("s.revision=NEW.to_revision"));
    assert!(
        VERIFIER.contains("BEFORE UPDATE OR DELETE ON scope_anti_bloat_preservation_attestations")
    );
    assert!(VERIFIER.contains("FORCE ROW LEVEL SECURITY"));
}
