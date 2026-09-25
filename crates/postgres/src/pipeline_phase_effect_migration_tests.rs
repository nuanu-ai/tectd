const APPLIED: &str = include_str!("../migrations/0074_pipeline_phase_effect_attestations.sql");
const FORWARD: &str =
    include_str!("../migrations/0075_pipeline_phase_effect_slice_origin_guard.sql");
const STORE: &str = include_str!("pipeline_phase_effect_store.rs");

fn verifier_body(sql: &str) -> &str {
    let start = sql
        .find("CREATE FUNCTION pipeline_phase_effect_require_verifier()")
        .or_else(|| sql.find("CREATE OR REPLACE FUNCTION pipeline_phase_effect_require_verifier()"))
        .expect("verifier function");
    let body = &sql[start..];
    let start = body.find("AS $guard$").expect("function body");
    let end = body.find("$guard$;").expect("function end") + "$guard$;".len();
    &body[start..end]
}

#[test]
fn forward_migration_changes_only_the_invalid_slice_erasure_predicate() {
    let old = verifier_body(APPLIED);
    let expected = old.replace(
        "AND NOT o.payload_erased AND NOT r.payload_erased AND NOT s.payload_erased",
        "AND NOT o.payload_erased AND NOT r.payload_erased AND s.origin_result IS NOT NULL",
    );
    assert_ne!(
        old, expected,
        "applied guard must contain the invalid predicate"
    );
    assert_eq!(verifier_body(FORWARD), expected);
    assert!(
        FORWARD.contains("CREATE OR REPLACE FUNCTION pipeline_phase_effect_require_verifier()")
    );
    assert!(FORWARD.contains(
        "REVOKE ALL PRIVILEGES ON FUNCTION pipeline_phase_effect_require_verifier() FROM PUBLIC"
    ));
}

#[test]
fn material_query_requires_saved_slice_origin_without_a_missing_column() {
    assert!(STORE.contains("AND s.origin_result IS NOT NULL"));
    assert!(!STORE.contains("AND NOT s.payload_erased"));
}
