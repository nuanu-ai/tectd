const SQL: &str = include_str!("../migrations/0097_provider_raw_observation.sql");
const PARTIAL: &str = include_str!("../migrations/0098_provider_partial_observation.sql");
const STORE: &str = include_str!("advisory/matrix_observation.rs");
const APP: &str = include_str!("../../application/src/matrix_advisory_dispatch.rs");
const RECOVERY: &str = include_str!("../../application/src/matrix_advisory_dispatch/recovery.rs");

#[test]
fn immutable_raw_observation_is_tenant_scoped_and_bound_to_committed_bytes() {
    for expected in [
        "FORCE ROW LEVEL SECURITY",
        "TG_OP <> 'INSERT'",
        "d.state='sending'",
        "d.configuration_digest=NEW.configuration_digest",
        "d.payload_digest=NEW.request_sha256",
        "pg_catalog.sha256(d.request_payload)",
        "pg_catalog.sha256(response_payload)",
        "response_complete",
        "partial_received",
        "original_input_tokens text",
        "elapsed_ms bigint",
    ] {
        assert!(SQL.contains(expected), "missing {expected}");
    }
    assert!(STORE.contains("continuation.actor_id()"));
    assert!(STORE.contains("reservation.policy_version"));
    assert!(STORE.contains("reservation.policy_digest"));
    assert!(STORE.contains("saved.raw_observation_sealed"));
    assert!(STORE.contains("saved.original_elapsed_ms != Some(elapsed)"));
    assert!(STORE.contains("saved.response_complete != observation.response_complete"));
    assert!(PARTIAL.contains("CREATE OR REPLACE FUNCTION advisory_provider_observation_guard()"));
    assert!(PARTIAL.contains("d.state='sending'"));
    assert!(PARTIAL.contains("d.payload_digest=NEW.request_sha256"));
    assert!(!PARTIAL.contains("NEW.response_complete=(NEW.response_payload IS NOT NULL)"));
}

#[test]
fn native_and_recovery_ranking_wait_for_committed_consumption_and_current_evidence() {
    let seal = APP.find(".seal_committed_matrix_observation(").unwrap();
    let usage = APP.find(".sealed_response_usage(").unwrap();
    let consume = APP.find(".consume_committed_matrix_observation(").unwrap();
    let parse = APP.find(".parse_sealed_response(").unwrap();
    assert!(seal < usage && usage < consume && consume < parse);
    assert!(!APP[seal..consume].contains(".authenticated("));
    let compact: String = APP.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert!(compact.contains("!consumption.exhausted_after_response&&!verification_stale"));
    assert!(APP.contains("config.revision == expected_config_revision"));
    assert!(
        RECOVERY
            .find(".consume_committed_matrix_observation(")
            .unwrap()
            < RECOVERY.find(".parse_sealed_response(").unwrap()
    );
    assert!(RECOVERY.contains("current && !exhausted"));
}
