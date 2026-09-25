const MIGRATION: &str = include_str!("../migrations/0089_advisory_budget_policy_global_ledger.sql");
const USAGE: &str = include_str!("budget_policy_usage.rs");
const ANTI_BLOAT: &str = include_str!("anti_bloat_store/send.rs");
const TWO_OPPORTUNITIES: &str = include_str!("../tests/sql/0089_two_opportunity_policy_budget.sql");

#[test]
fn exact_policy_serialization_and_cross_route_ledger_are_required() {
    for source in [MIGRATION, USAGE] {
        assert!(source.contains("advisory_budget_reservations"));
        assert!(source.contains("scope_anti_bloat_budget_reservations"));
        assert!(source.contains("advisory_budget_consumptions"));
        assert!(source.contains("scope_anti_bloat_budget_consumptions"));
        assert!(source.contains("policy_version"));
        assert!(source.contains("policy_digest"));
    }
    assert!(MIGRATION.contains("FOR UPDATE"));
    assert!(MIGRATION.contains("u.calls+1>p.provider_calls"));
    assert!(MIGRATION.contains("u.request_bytes+NEW.request_utf8_bytes>p.request_utf8_bytes"));
    assert!(MIGRATION.contains("u.retries+attempt_retries>p.retry_dispatches"));
    assert!(MIGRATION.contains("u.pending<>0 OR u.invalid<>0"));
    assert!(USAGE.contains("FOR UPDATE"));
    assert!(ANTI_BLOAT.contains("crate::budget_policy_usage::policy_usage("));
    assert!(TWO_OPPORTUNITIES.contains("second opportunity improperly passed one-call policy"));
    assert!(TWO_OPPORTUNITIES.contains("two-call policy did not admit two opportunities"));
    assert!(TWO_OPPORTUNITIES.contains("shared dispatch ignored Anti-Bloat call"));
}
