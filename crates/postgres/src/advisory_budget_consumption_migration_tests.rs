const MIGRATION: &str = include_str!("../migrations/0085_advisory_budget_consumption.sql");
const START: &str = include_str!("advisory/dispatch/budget_reservation.rs");
const SEAL: &str = include_str!("advisory/dispatch/lifecycle.rs");
const SCOPE: &str = include_str!("../../application/src/scope_advisory_orchestration/dispatch.rs");
const MATRIX: &str = include_str!("../../application/src/matrix_advisory_dispatch.rs");
const SCOPE_FINALIZE: &str = include_str!("scope_advisory/finalize.rs");
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
        "budget_exhausted_after_response",
        "FORCE ROW LEVEL SECURITY",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(START.contains("usage.pending != 0"));
    assert!(START.contains("reserved_input_tokens: remaining_input"));
    assert!(START.contains("reserved_output_tokens: remaining_output"));
    assert!(SEAL.contains("Some((_, None)) => return Err(Error::BudgetPolicyInvalid)"));
    assert!(SCOPE_FINALIZE.contains("NOT b.unknown_usage AND NOT b.exhausted_after_response"));
    assert!(AUTHORIZE.contains("AdvisoryReason::BudgetExhaustedAfterResponse"));
    assert!(CONSUME.contains("if let Some(saved) = consumed_budget"));
    assert!(
        CONSUME.contains("opportunity_by_id(tx, tenant, workspace, dispatch.opportunity_id, true)")
    );
    assert!(CONSUME.contains("crate::budget_policy_usage::policy_usage("));
    assert!(CONSUME.contains("usage.pending != 1"));
    {
        let seal_commit = SCOPE.find("seal_tx.commit().await?").unwrap();
        let consume = SCOPE.find(".consume_advisory_budget(").unwrap();
        assert!(seal_commit < consume);
        assert!(SCOPE.contains("monotonic_start.elapsed().as_millis()"));
    }
    let raw = MATRIX.find(".seal_committed_matrix_observation(").unwrap();
    let usage = MATRIX.find(".sealed_response_usage(").unwrap();
    let consume = MATRIX
        .find(".consume_committed_matrix_observation(")
        .unwrap();
    let parse = MATRIX.find(".parse_sealed_response(").unwrap();
    assert!(raw < usage && usage < consume && consume < parse);
    assert!(MATRIX.contains("!consumption.exhausted_after_response"));
}
