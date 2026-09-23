const MIGRATION: &str = include_str!("../migrations/0040_scope_advisory_persistence.sql");
const MANIFEST: &str = include_str!("scope_advisory/manifest.rs");
const DECISIONS: &str = include_str!("scope_advisory/decisions.rs");
const FINALIZE: &str = include_str!("scope_advisory/finalize.rs");
const MAPPINGS: &str = include_str!("scope_advisory/mappings.rs");
const ADMIN_VALIDATOR: &str = include_str!("admin/scope_advisory.rs");
const PORT: &str = include_str!("../../application/src/scope_advisory_ports.rs");

#[test]
fn migration_has_exactly_seven_authoritative_aggregates_and_no_budget_placeholder() {
    for table in [
        "advisory_scope_source_snapshot",
        "advisory_scope_manifest",
        "advisory_scope_advice",
        "advisory_scope_disposition",
        "advisory_scope_preservation_receipt",
        "advisory_scope_caller_link",
        "advisory_scope_verifier_receipt",
    ] {
        assert!(MIGRATION.contains(&format!("CREATE TABLE {table} (")));
    }
    assert_eq!(MIGRATION.matches("CREATE TABLE advisory_").count(), 7);
    for forbidden in [
        "advisory_budget_",
        "advisory_scope_source_input",
        "advisory_scope_obligation",
        "advisory_scope_alternative",
        "advisory_scope_coverage",
        "advisory_scope_advice_item",
        "advisory_scope_disposition_item",
        "CREATE TABLE advisory_event",
    ] {
        assert!(!MIGRATION.contains(forbidden), "unexpected {forbidden}");
    }
}

#[test]
fn aggregates_are_canonical_json_with_relational_case_and_digest_lineage() {
    for schema in [
        "tect.scope-source-obligations/1",
        "tect.scope-constructor-manifest/2",
        "tect.guarded-scope-advice/1",
        "tect.scope-disposition-revision/1",
        "tect.scope-preservation/1",
    ] {
        assert!(MIGRATION.contains(schema));
    }
    for relation in [
        "advisory_scope_source_opportunity_fk",
        "advisory_scope_source_case_fk",
        "advisory_scope_manifest_source_fk",
        "advisory_scope_advice_manifest_fk",
        "advisory_scope_advice_dispatch_fk",
        "advisory_scope_disposition_advice_fk",
        "advisory_scope_disposition_predecessor_fk",
        "advisory_scope_preservation_disposition_fk",
        "advisory_scope_preservation_manifest_fk",
        "advisory_scope_caller_preservation_fk",
        "advisory_scope_caller_receipt_fk",
        "advisory_scope_verifier_caller_fk",
    ] {
        assert!(MIGRATION.contains(relation), "missing {relation}");
    }
    assert!(MIGRATION.matches("case_id").count() > 20);
}

#[test]
fn every_aggregate_write_and_read_is_domain_validated() {
    for token in [
        "record.manifest.validate(&Sha256ScopeDigest)",
        "manifest.validate(&Sha256ScopeDigest)",
        "validate_guarded_advice_binding(&Sha256ScopeDigest",
        "revision.validate(&Sha256ScopeDigest",
        "evaluate_scope_preservation(",
    ] {
        assert!(
            MANIFEST.contains(token) || MAPPINGS.contains(token) || DECISIONS.contains(token),
            "missing validation {token}"
        );
    }
    assert!(MAPPINGS.contains("serde_json::from_value"));
    assert!(DECISIONS.contains("serde_json::to_value"));
}

#[test]
fn lifecycle_cas_and_independence_guards_are_explicit() {
    for token in [
        "state='prepared'",
        "decision_point='scope.decomposition.before_selection'",
        "d.state='sealed'",
        "d.send_certainty='sent'",
        "d.outcome='provider_response'",
        "o.state='advised'",
        "lock_scope_key(",
        "ORDER BY revision DESC LIMIT 1",
        "request.into_revision",
        "status='passed'",
        "input.actor_id == caller_actor && input.session_id == caller_session",
        "input.actor_id == decision_actor && input.session_id == decision_session",
    ] {
        assert!(
            MANIFEST.contains(token) || DECISIONS.contains(token),
            "missing {token}"
        );
    }
    for token in [
        "advisory_scope_disposition_one_root_unique",
        "advisory_scope_disposition_one_successor_unique",
        "predecessor_revision=revision-1",
    ] {
        assert!(MIGRATION.contains(token));
    }
    assert!(MIGRATION.contains("verified_revision>=1"));
    assert!(!MIGRATION.contains("verified_revision>=3"));
}

#[test]
fn current_config_guard_and_replay_lineage_are_explicit() {
    for token in [
        "require_current_opportunity_config(",
        "c.mode='optional'",
        "c.provider_profile_ref=h.provider_profile_ref",
        "c.model_configuration=h.model_configuration",
        "Err(Error::StaleContext)",
        "row.opportunity_id != record.opportunity_id",
        "row.case_id != record.case_id",
        "preservation_receipt_id",
    ] {
        assert!(
            MAPPINGS.contains(token) || DECISIONS.contains(token),
            "missing {token}"
        );
    }
}

#[test]
fn guarded_advice_and_advised_transition_share_one_transaction() {
    for token in [
        "SET state='advised'",
        "o.state='awaiting_response'",
        "d.send_certainty='sent'",
        "d.outcome='provider_response'",
        "persist_advice(tx, tenant, workspace, record).await",
    ] {
        assert!(FINALIZE.contains(token), "atomic finalize misses {token}");
    }
    assert!(FINALIZE.contains("SET state='invalidated'"));
    assert!(PORT.contains("finalize_guarded_scope_advice"));
    assert!(PORT.contains("invalidate_scope_advisory"));
}

#[test]
fn startup_validator_checks_specific_schema_contract() {
    for token in [
        "advisory_scope_source_candidate_fk",
        "advisory_scope_source_snapshot_fk",
        "advisory_scope_disposition_actor_fk",
        "advisory_scope_disposition_session_fk",
        "advisory_scope_caller_actor_fk",
        "advisory_scope_verifier_session_fk",
        "advisory_scope_source_schema_check",
        "advisory_scope_advice_payload_check",
        "advisory_scope_disposition_chain_unique",
        "advisory_scope_disposition_one_successor_unique",
        "advisory_scope_verifier_digest_check",
        "policyname=tablename||'_tenant_scope'",
        "qual=with_check",
        "has_table_privilege($1,pg_catalog.format('public.%I',table_name),'TRIGGER')",
    ] {
        assert!(ADMIN_VALIDATOR.contains(token), "validator misses {token}");
    }
}

#[test]
fn runtime_contract_is_append_only_tenant_safe_and_port_only() {
    for token in [
        "ENABLE ROW LEVEL SECURITY",
        "FORCE ROW LEVEL SECURITY",
        "FROM PUBLIC",
        "advisory_scope_case_lookup_idx",
        "advisory_scope_disposition_lookup_idx",
        "advisory_scope_caller_lookup_idx",
    ] {
        assert!(MIGRATION.contains(token));
    }
    assert!(!MIGRATION.contains("GRANT UPDATE"));
    assert!(!MIGRATION.contains("GRANT DELETE"));
    for method in [
        "prepare_scope_advisory_manifest",
        "scope_advisory_manifest",
        "persist_guarded_scope_advice",
        "cas_scope_advisory_disposition",
        "persist_scope_preservation_receipt",
        "link_scope_advisory_caller",
        "persist_scope_verifier_receipt",
    ] {
        assert!(PORT.contains(method));
    }
    for source in [PORT, MANIFEST, DECISIONS, MAPPINGS] {
        assert!(!source.contains("raw_response"));
        assert!(!source.contains("response_payload bytea"));
    }
}
