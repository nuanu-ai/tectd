const SQL: &str = include_str!("../migrations/0108_matrix_v1_dispatch_cutover_allowlist.sql");

#[test]
fn matrix_v1_cutover_is_locked_exact_and_read_only() {
    assert!(SQL.contains("LOCK TABLE public.advisory_dispatch IN ACCESS EXCLUSIVE MODE;"));
    assert!(SQL.contains("IN SHARE ROW EXCLUSIVE MODE"));
    assert!(SQL.contains("CREATE TABLE public.matrix_v1_dispatch_cutover_allowlist"));
    assert!(SQL.contains("AND v.schema='tect.matrix-verification/1'"));
    assert!(SQL.contains("AND d.state='sealed' AND d.send_certainty='sent'"));
    assert!(SQL.contains("AND d.outcome='provider_response'"));
    assert!(SQL.contains("AND p.original_transport_outcome='received' AND p.response_complete"));
    assert!(SQL.contains("AND p.response_payload=d.response_payload"));
    assert!(SQL.contains("AND NOT EXISTS ("));
    assert!(SQL.contains("BEFORE INSERT OR UPDATE OR DELETE"));
    assert!(SQL.contains("BEFORE INSERT ON public.advisory_matrix_advice"));
    assert!(SQL.contains("public.matrix_v1_cutover_fingerprint("));
    assert!(SQL.contains("FORCE ROW LEVEL SECURITY"));
    assert!(
        SQL.contains("proves database presence at cutover, not an external call")
            || SQL.contains("This proves database presence at cutover, not an external call")
    );
}
