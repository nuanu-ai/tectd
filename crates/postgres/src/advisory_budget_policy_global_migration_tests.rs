const MIGRATION: &str = include_str!("../migrations/0089_advisory_budget_policy_global_ledger.sql");
const USAGE: &str = include_str!("budget_policy_usage.rs");
const ANTI_BLOAT: &str = include_str!("anti_bloat_store/send.rs");
const TWO_OPPORTUNITIES: &str = include_str!("../tests/sql/0089_two_opportunity_policy_budget.sql");
const LOCK_MIGRATION: &str =
    include_str!("../migrations/0090_advisory_budget_policy_immutable_lock.sql");
const GRANTS: &str = include_str!("admin/migration.rs");
const LOCK_TEST: &str = include_str!("../tests/sql/0090_policy_lock_privilege.sql");

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
    let begin = ANTI_BLOAT.find("pub(super) async fn begin_send").unwrap();
    let opportunity_lock = ANTI_BLOAT[begin..]
        .find("SELECT id FROM advisory_opportunity")
        .unwrap()
        + begin;
    let policy_lock = ANTI_BLOAT[begin..]
        .find("crate::budget_policy_usage::policy_usage(")
        .unwrap()
        + begin;
    assert!(opportunity_lock < policy_lock);
    assert!(!ANTI_BLOAT[begin..policy_lock].contains("FROM advisory_budget_policies"));
    assert!(TWO_OPPORTUNITIES.contains("second opportunity improperly passed one-call policy"));
    assert!(TWO_OPPORTUNITIES.contains("two-call policy did not admit two opportunities"));
    assert!(TWO_OPPORTUNITIES.contains("shared dispatch ignored Anti-Bloat call"));
    assert!(LOCK_MIGRATION.contains("ENABLE ALWAYS TRIGGER advisory_budget_policy_guard_trigger"));
    assert!(GRANTS.contains("GRANT UPDATE(id) ON TABLE advisory_budget_policies"));
    assert!(LOCK_TEST.contains("FOR UPDATE"));
    assert!(LOCK_TEST.contains("advisory budget policy is immutable"));
}
