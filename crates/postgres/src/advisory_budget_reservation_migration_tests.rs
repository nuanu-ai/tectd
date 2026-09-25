const MIGRATION: &str = include_str!("../migrations/0084_advisory_budget_reservations.sql");
const START: &str = include_str!("advisory/dispatch/lifecycle.rs");
const GUARD: &str = include_str!("advisory/dispatch/budget_reservation.rs");

#[test]
fn reservation_is_bound_to_dispatch_policy_bytes_and_tenant() {
    for clause in [
        "CREATE TABLE advisory_budget_reservations",
        "PRIMARY KEY (tenant_id, workspace_id, dispatch_id)",
        "REFERENCES advisory_dispatch (tenant_id,workspace_id,opportunity_id,id)",
        "REFERENCES advisory_budget_policies (tenant_id,workspace_id,id)",
        "NEW.request_utf8_bytes <> pg_catalog.octet_length(dispatch.request_payload)",
        "pg_catalog.sha256(dispatch.request_payload)",
        "NEW.policy_effective_from_unix_ms <> policy.effective_from_unix_ms",
        "NEW.policy_effective_until_unix_ms <> policy.effective_until_unix_ms",
        "TG_OP <> 'INSERT'",
        "FORCE ROW LEVEL SECURITY",
    ] {
        assert!(MIGRATION.contains(clause), "missing {clause}");
    }
    let reserve = START.find("reserve_before_dispatch(tx").unwrap();
    let send = START
        .find("UPDATE advisory_dispatch SET state='sending'")
        .unwrap();
    assert!(reserve < send);
    assert!(GUARD.contains("policy.ok_or(Error::BudgetPolicyInvalid)?"));
    assert!(GUARD.contains("opportunity_id=$3"));
    assert!(GUARD.contains("foreign_policy != 0"));
}
