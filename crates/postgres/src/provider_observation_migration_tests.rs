const SQL: &str = include_str!("../migrations/0097_provider_raw_observation.sql");
const PARTIAL: &str = include_str!("../migrations/0098_provider_partial_observation.sql");
const STORE: &str = include_str!("advisory/provider_observation.rs");
const FAMILIES: &str = include_str!("../migrations/0099_provider_observation_families.sql");
const CONTEXT: &str = include_str!("../migrations/0100_provider_transport_context.sql");
const APP: &str = include_str!("../../application/src/matrix_advisory_dispatch.rs");
const RECOVERY: &str = include_str!("../../application/src/matrix_advisory_dispatch/recovery.rs");

#[test]
fn pipeline_receipt_and_interpretation_forward_contracts_preserve_transport() {
    let latest = include_str!("../migrations/0103_pipeline_interpretation_option_ids.sql");
    let renamed = include_str!("../migrations/0072_pipeline_verification_plan_binding.sql");
    assert!(renamed.contains("RENAME COLUMN eligible_kind_ids TO eligible_option_ids"));
    assert!(latest.contains("'context.eligible_kind_ids','context.eligible_option_ids'"));
    assert!(latest.contains("pg_catalog.pg_get_functiondef"));
    let raw = include_str!("../migrations/0101_pipeline_provider_receipt_seal.sql");
    let interpreted = include_str!("../migrations/0102_pipeline_advice_interpretations.sql");
    for expected in [
        "old_arm",
        "r.response_payload IS NOT DISTINCT FROM NEW.response_payload",
        "r.elapsed_ms=NEW.latency_ms",
        "BETWEEN 0 AND 65536",
        "NEW.pipeline_response_sha256",
        "NEW.input_tokens IS NULL AND NEW.output_tokens IS NULL",
    ] {
        assert!(raw.contains(expected), "{expected}");
    }
    for expected in [
        "contract_version=1",
        "FORCE ROW LEVEL SECURITY",
        "REFERENCES advisory_dispatch",
        "pipeline interpretation is immutable",
        "NOT c.unknown_usage",
        "NOT c.exhausted_after_response",
        "i.response_sha256=encode(sha256(saved_bytes)",
        "ELSE\n            ranking := pg_catalog.convert_from(saved_bytes",
    ] {
        assert!(interpreted.contains(expected), "{expected}");
    }
    let adapter = include_str!("pipeline_recommendation_store/interpretation.rs");
    let compact: String = adapter.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(compact.contains("value.ranking.validate(&saved.manifest)"));
    assert!(adapter.contains("stored != *value"));
    assert!(!adapter.contains("tect_host"));
    let attached = raw.find("AND EXISTS (").unwrap();
    let fail_closed = raw
        .find("pipeline seal differs from committed raw receipt")
        .unwrap();
    let legacy = raw.find("$arm$ || old_arm").unwrap();
    assert!(attached < fail_closed && fail_closed < legacy);
    assert!(
        interpreted.contains("IF EXISTS (SELECT 1 FROM public.pipeline_advice_interpretations")
    );
    assert!(interpreted.contains("IF ranking IS NULL THEN RAISE EXCEPTION"));
}

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
    assert!(STORE.contains("existing != observation"));
    assert!(STORE.contains("saved.original_elapsed_ms != Some(elapsed)"));
    assert!(!STORE.contains("matrix_task_revisions"));
    assert!(!STORE.contains(".authenticated("));
    for binding in [
        "opportunity.target_kind != continuation.target_kind()",
        "opportunity.target_id != continuation.target_id()",
        "opportunity.work_revision != continuation.work_revision()",
        "opportunity.material_digest != continuation.material_digest()",
    ] {
        assert!(STORE.contains(binding));
    }
    for family in [
        "('engineering_profile','engineering.profile.before_selection','matrix_task')",
        "('scope_decomposition','scope.decomposition.before_selection','scope_candidate_set')",
        "('pipeline_recommendation','pipeline_recommendation_before_slice_open','slice_candidate_node')",
    ] {
        assert!(FAMILIES.contains(family));
    }
    assert!(PARTIAL.contains("CREATE OR REPLACE FUNCTION advisory_provider_observation_guard()"));
    assert!(PARTIAL.contains("d.state='sending'"));
    assert!(PARTIAL.contains("d.payload_digest=NEW.request_sha256"));
    assert!(!PARTIAL.contains("NEW.response_complete=(NEW.response_payload IS NOT NULL)"));
    assert!(CONTEXT.contains("ADD COLUMN original_transport_context jsonb"));
    assert!(!CONTEXT.contains("CREATE TABLE"));
    assert!(!CONTEXT.contains("CREATE OR REPLACE"));
    assert!(STORE.contains("apply_scope_transport_context"));
    assert!(STORE.contains("seal.raw_response_ref = context.raw_response_ref.clone()"));
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
