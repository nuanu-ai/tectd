const MIGRATION: &str = include_str!("../migrations/0062_advisory_policy_ledger_guards.sql");
const ANTI_GLOBAL: &str =
    include_str!("../migrations/0111_anti_bloat_shared_global_budget_ledger.sql");
const USAGE: &str = include_str!("budget_policy_usage.rs");
const START: &str = include_str!("advisory/dispatch/budget_reservation.rs");
const LOCK_MIGRATION: &str = include_str!("../migrations/0059_advisory_budget_policy.sql");
const GRANTS: &str = include_str!("admin/migration/matrix_advice.rs");

#[test]
fn exact_policy_serialization_and_deployed_advisory_ledger_are_required() {
    for source in [MIGRATION, USAGE] {
        for required in [
            "advisory_budget_reservations",
            "advisory_budget_consumptions",
            "policy_version",
            "policy_digest",
            "FOR UPDATE",
        ] {
            assert!(source.contains(required), "missing {required}");
        }
    }
    // The installed legacy migration retains its original advisory-only shape.
    for foreign in [
        "scope_anti_bloat_budget_reservations",
        "model_route_budget_reservations",
    ] {
        assert!(
            !MIGRATION.contains(foreign),
            "legacy foreign ledger {foreign}"
        );
    }
    // Current producers and the forward SQL ledger include each purpose once.
    for source in [USAGE, ANTI_GLOBAL] {
        for ledger in [
            "advisory_budget_reservations",
            "scope_anti_bloat_budget_reservations",
            "model_route_budget_reservations",
        ] {
            assert!(source.contains(ledger), "missing current ledger {ledger}");
        }
        for consumption in [
            "advisory_budget_consumptions",
            "scope_anti_bloat_budget_consumptions",
            "model_route_budget_consumptions",
        ] {
            assert!(
                source.contains(consumption),
                "missing current consumption {consumption}"
            );
        }
        assert_eq!(source.matches("UNION ALL").count(), 2);
    }
    assert!(ANTI_GLOBAL.contains("CREATE OR REPLACE FUNCTION advisory_budget_policy_usage_totals"));
    for (trigger, guard) in [
        (
            "zz_anti_bloat_budget_global_reservation_guard",
            "advisory_budget_global_reservation_guard",
        ),
        (
            "zz_anti_bloat_budget_global_consumption_guard",
            "advisory_budget_global_consumption_guard",
        ),
    ] {
        assert!(ANTI_GLOBAL.contains(&format!("CREATE TRIGGER {trigger}")));
        assert!(ANTI_GLOBAL.contains(&format!("EXECUTE FUNCTION {guard}();")));
        assert!(MIGRATION.contains(&format!("CREATE FUNCTION {guard}()")));
    }
    for required in [
        "u.calls+1>p.provider_calls",
        "u.request_bytes+NEW.request_utf8_bytes>p.request_utf8_bytes",
        "u.retries+attempt_retries>p.retry_dispatches",
        "u.pending<>0 OR u.invalid<>0",
        "u.pending<>1",
        "transaction_isolation",
        "read committed",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    let route_lock = START.find("opportunity_id=$3").unwrap();
    let policy_lock = START
        .find("crate::budget_policy_usage::policy_usage(")
        .unwrap();
    assert!(route_lock < policy_lock);
    // Source-shape proof of the actual inline guard, not deployed SQL behavior.
    let workspace_serialization = START.find("lock_workspace_policy(").unwrap();
    let database_clock = START
        .find("SELECT (EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())*1000)::bigint")
        .unwrap();
    let installed_policy = START.find("let installed:").unwrap();
    let policy_comparison = START.find("if installed.as_ref().is_none_or").unwrap();
    assert!(workspace_serialization < database_clock);
    assert!(database_clock < installed_policy);
    assert!(installed_policy < policy_comparison);
    assert!(policy_comparison < policy_lock);
    let currentness = &START[workspace_serialization..policy_lock];
    for required in [
        "if !policy.is_effective_at(now)",
        "SELECT id,version,digest,effective_from_unix_ms,effective_until_unix_ms",
        "WHERE tenant_id=$1 AND workspace_id=$2",
        "effective_from_unix_ms<=$3 AND effective_until_unix_ms>$3",
        "ORDER BY version DESC LIMIT 1",
        ".bind(now)",
        "current.0 != policy.id()",
        "current.1 != policy.version()",
        "current.2 != policy.digest()",
        "current.3 != policy.effective_from_unix_ms()",
        "current.4 != policy.effective_until_unix_ms()",
        "return Err(Error::BudgetPolicyInvalid);",
    ] {
        assert!(
            currentness.contains(required),
            "missing inline guard {required}"
        );
    }
    assert!(LOCK_MIGRATION.contains("ENABLE ALWAYS TRIGGER advisory_budget_policy_guard_trigger"));
    assert!(GRANTS.contains("GRANT UPDATE(id) ON TABLE advisory_budget_policies"));
}
