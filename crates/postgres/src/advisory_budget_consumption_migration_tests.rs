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
    assert!(START.contains("c.dispatch_id IS NULL"));
    assert!(START.contains("reserved_input_tokens: remaining_input"));
    assert!(START.contains("reserved_output_tokens: remaining_output"));
    assert!(SEAL.contains("Some((_, None)) => return Err(Error::BudgetPolicyInvalid)"));
    assert!(SCOPE_FINALIZE.contains("NOT b.unknown_usage AND NOT b.exhausted_after_response"));
    assert!(AUTHORIZE.contains("AdvisoryReason::BudgetExhaustedAfterResponse"));
    assert!(CONSUME.contains("if let Some(saved) = consumed_budget"));
    assert!(
        CONSUME.contains("opportunity_by_id(tx, tenant, workspace, dispatch.opportunity_id, true)")
    );
    assert!(
        CONSUME.contains("COUNT(*) FILTER (WHERE r.dispatch_id<>$4 AND c.dispatch_id IS NULL)")
    );
    for path in [SCOPE, MATRIX] {
        let seal_commit = path.find("seal_tx.commit().await?").unwrap();
        let consume = path.find(".consume_advisory_budget(").unwrap();
        assert!(seal_commit < consume);
        assert!(path.contains("monotonic_start.elapsed().as_millis()"));
    }
}
