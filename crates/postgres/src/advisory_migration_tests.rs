const MIGRATION: &str = include_str!("../migrations/0039_advisory_optionality_audit.sql");

#[test]
fn advisory_migration_declares_the_exact_four_table_spine() {
    assert_eq!(MIGRATION.matches("CREATE TABLE advisory_").count(), 4);
    for table in [
        "advisory_workspace_config",
        "advisory_workspace_config_history",
        "advisory_opportunity",
        "advisory_dispatch",
    ] {
        assert!(MIGRATION.contains(&format!("CREATE TABLE {table} (")));
        assert!(MIGRATION.contains(&format!("ALTER TABLE {table} ENABLE ROW LEVEL SECURITY")));
        assert!(MIGRATION.contains(&format!("ALTER TABLE {table} FORCE ROW LEVEL SECURITY")));
    }
    assert!(!MIGRATION.contains("CREATE TABLE advisory_opportunities"));
    assert!(!MIGRATION.contains("CREATE TABLE advisory_dispatches"));
}

#[test]
fn advisory_migration_closes_states_retries_and_capabilities() {
    for token in [
        "'model_routing'",
        "'prepared', 'no_call', 'awaiting_response', 'advised'",
        "'authorized', 'sending', 'sealed', 'cancelled'",
        "'not_sent', 'sent', 'sent_unknown'",
        "'initial', 'proven_not_sent', 'known_retryable_response'",
        "'verified_provider_idempotency'",
        "revision = 0 AND previous_revision IS NULL",
        "previous_revision = revision - 1",
        "decision_point = 'scope.decomposition.before_selection'",
        "capability = 'scope_decomposition'",
        "model_configuration - 'model' = '{}'::jsonb",
        "pg_catalog.btrim(provider_profile_ref) = provider_profile_ref",
        "pg_catalog.btrim(model_configuration->>'model') = model_configuration->>'model'",
    ] {
        assert!(
            MIGRATION.contains(token),
            "missing schema contract: {token}"
        );
    }
    assert!(!MIGRATION.contains("model_recommendation"));
    assert!(!MIGRATION.contains("confirmed_not_sent"));
    assert!(!MIGRATION.contains("api_key"));
    assert!(!MIGRATION.contains("credential"));
}

#[test]
fn advisory_migration_has_tenant_safe_links_and_required_indexes() {
    for token in [
        "advisory_opportunity_tenant_id_unique UNIQUE (tenant_id, workspace_id, id)",
        "REFERENCES advisory_opportunity (tenant_id, workspace_id, id)",
        "UNIQUE (tenant_id, workspace_id, opportunity_id, id)",
        "UNIQUE (tenant_id, workspace_id, opportunity_id, attempt_number)",
        "FOREIGN KEY (tenant_id, workspace_id, opportunity_id, predecessor_dispatch_id)",
        "REFERENCES advisory_dispatch (tenant_id, workspace_id, opportunity_id, id)",
        "advisory_workspace_config_history_predecessor_fk",
        "advisory_workspace_config_history_fk",
        "CREATE INDEX advisory_opportunity_workspace_audit_idx",
        "CREATE INDEX advisory_opportunity_scope_audit_idx",
        "CREATE INDEX advisory_dispatch_unresolved_idx",
        "WHERE state = 'sending' AND send_certainty = 'sent_unknown'",
        "REVOKE ALL PRIVILEGES ON TABLE advisory_workspace_config",
    ] {
        assert!(
            MIGRATION.contains(token),
            "missing schema contract: {token}"
        );
    }
}

#[test]
fn packet_b_adapter_uses_authoritative_config_cas_and_opportunity_contract() {
    let adapter = include_str!("advisory.rs");
    for token in [
        "FROM advisory_workspace_config",
        "INSERT INTO advisory_workspace_config_history",
        "revision,previous_revision",
        "ON CONFLICT DO NOTHING",
        "FOR UPDATE",
        "AND revision=$9",
        "INSERT INTO advisory_opportunity",
        "ON CONFLICT(tenant_id,workspace_id,request_key) DO NOTHING",
        "row.material_digest != input.material_digest",
        "current_revision != input.config_revision",
    ] {
        assert!(
            adapter.contains(token),
            "missing Packet B adapter contract: {token}"
        );
    }
    for obsolete in [
        "workspace_advisory_configs",
        "workspace_advisory_config_history",
        "advisory_opportunities",
        "advisory_dispatches",
    ] {
        assert!(
            !adapter.contains(obsolete),
            "obsolete draft schema: {obsolete}"
        );
    }
}

#[test]
fn packet_c_adapter_persists_each_dispatch_boundary_and_freshness_check() {
    let adapter = include_str!("advisory.rs");
    for token in [
        "INSERT INTO advisory_dispatch",
        "state='sending',send_certainty='sent_unknown'",
        "send_started_at=pg_catalog.clock_timestamp()",
        "WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='authorized'",
        "state='sealed'",
        "sealed_at=pg_catalog.clock_timestamp()",
        "dispatch_matches_authorization",
        "advisory_retry_permitted",
        "current.0 != expected_config_revision",
        "AdvisoryOpportunityState::Invalidated",
        "AdvisoryOpportunityState::Unresolved",
    ] {
        assert!(
            adapter.contains(token),
            "missing Packet C adapter contract: {token}"
        );
    }
}

#[test]
fn packet_c_cancel_reconcile_and_retry_are_fail_closed() {
    let migration = include_str!("../migrations/0039_advisory_optionality_audit.sql");
    let adapter = include_str!("advisory.rs");
    for token in [
        "advisory_dispatch_retry_child_unique",
        "WHERE predecessor_dispatch_id IS NOT NULL",
        "retry_basis = 'proven_not_sent'",
    ] {
        assert!(
            migration.contains(token),
            "missing retry schema guard: {token}"
        );
    }
    for token in [
        "AdvisoryCancellationOutcome::DeliveryMayHaveOccurred",
        "dispatch_by_id(tx, tenant, workspace, dispatch_id, true)",
        "id=$3 AND state='authorized'",
        "AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed => false",
        "AdvisoryReconciliationEvidence::Inconclusive",
        "AdvisoryReconciliationEvidence::ConfirmedSent",
        "AdvisoryReconciliationEvidence::ConfirmedNotSent",
        "state='sending' AND send_certainty='sent_unknown'",
        "previous.material_digest != input.material_digest",
        "previous.payload_digest != input.payload_digest",
        "child_exists",
        "AdvisoryDispatchState::Cancelled | AdvisoryDispatchState::Sealed",
    ] {
        assert!(adapter.contains(token), "missing lifecycle guard: {token}");
    }
}

#[test]
fn packet_d_audit_is_scoped_stable_batched_and_truthful() {
    let adapter = include_str!("advisory.rs");
    let audit = adapter
        .split_once("async fn audit(")
        .expect("audit function")
        .1
        .split_once("async fn opportunity_detail(")
        .expect("detail boundary")
        .0;
    for token in [
        "o.tenant_id=$1 AND o.workspace_id=$2",
        "($3::uuid IS NULL OR o.scope_id=$3)",
        "(o.created_at,o.id)<",
        "ORDER BY o.created_at DESC,o.id DESC",
        "LIMIT $9",
        "d.opportunity_id=ANY($3)",
        "send_certainty='sent_unknown'",
        "send_certainty='not_sent' AND d.state IN ('sealed','cancelled')",
        "attempts_with_unknown_token_usage",
        "GROUP BY o.primary_reason ORDER BY o.primary_reason",
        "octet_length(d.request_payload)::bigint AS request_bytes",
    ] {
        assert!(
            audit.contains(token),
            "missing Packet D audit contract: {token}"
        );
    }
    assert!(!audit.contains("configuration_snapshot"));
    assert!(!audit.contains("response_payload,d.input_tokens"));
    assert_eq!(audit.matches("d.opportunity_id=ANY($3)").count(), 1);

    let detail = adapter
        .split_once("async fn opportunity_detail(")
        .expect("detail function")
        .1;
    assert!(detail.contains("o.scope_id=$3 AND o.id=$4"));
    assert!(detail.contains("ORDER BY d.attempt_number,d.id"));
}
