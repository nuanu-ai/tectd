const MIGRATION: &str = include_str!("../migrations/0061_advisory_budget_consumptions.sql");
const REASONS: &str = include_str!("../migrations/0058_matrix_advisory_verification_reasons.sql");
const START: &str = include_str!("advisory/dispatch/budget_reservation.rs");
const SEAL: &str = include_str!("advisory/dispatch/finalization.rs");
const MATRIX: &str = include_str!("../../application/src/matrix_advisory_dispatch.rs");
const AUTHORIZE: &str = include_str!("advisory/dispatch/authorization.rs");
const CONSUME: &str = include_str!("advisory/dispatch/budget_consumption.rs");

#[test]
fn consumption_is_append_only_bound_to_seal_and_gates_finalization() {
    for required in [
        "CREATE TABLE advisory_budget_consumptions",
        "PRIMARY KEY (tenant_id,workspace_id,dispatch_id)",
        "REFERENCES advisory_budget_reservations",
        "dispatch.state <> 'sealed'",
        "NEW.response_sha256 IS DISTINCT FROM",
        "NEW.input_tokens IS DISTINCT FROM dispatch.input_tokens",
        "NEW.monotonic_elapsed_ms IS DISTINCT FROM dispatch.latency_ms",
        "TG_OP <> 'INSERT'",
        "FORCE ROW LEVEL SECURITY",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(REASONS.contains("budget_exhausted_after_response"));
    assert!(START.contains("usage.pending != 0"));
    assert!(START.contains("reserved_input_tokens: remaining_input"));
    assert!(START.contains("reserved_output_tokens: remaining_output"));
    assert!(SEAL.contains("Some((_, None)) => return Err(Error::BudgetPolicyInvalid)"));
    assert!(AUTHORIZE.contains("AdvisoryReason::BudgetExhaustedAfterResponse"));
    assert!(CONSUME.contains("if let Some(saved) = consumed_budget"));
    assert!(
        CONSUME.contains("opportunity_by_id(tx, tenant, workspace, dispatch.opportunity_id, true)")
    );
    assert!(CONSUME.contains("crate::budget_policy_usage::policy_usage("));
    assert!(CONSUME.contains("usage.pending != 1"));
    let raw = MATRIX.find(".seal_committed_matrix_observation(").unwrap();
    let usage = MATRIX.find(".sealed_response_usage(").unwrap();
    let consume = MATRIX
        .find(".consume_committed_matrix_observation(")
        .unwrap();
    let parse = MATRIX.find(".parse_sealed_response(").unwrap();
    assert!(raw < usage && usage < consume && consume < parse);
    assert!(MATRIX.contains("!consumption.exhausted_after_response"));
}
