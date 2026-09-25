const MIGRATION: &str = include_str!("../migrations/0080_model_route_recommendation_receipts.sql");
const STORE: &str = include_str!("model_route_store.rs");
const GRANTS: &str = include_str!("admin/migration.rs");

#[test]
fn receipts_are_tenant_scoped_immutable_and_bound_to_exact_native_save() {
    for required in [
        "CREATE TABLE model_route_preparations",
        "REFERENCES matrix_planning_selection_links",
        "(tenant_id,workspace_id,candidate_set_id,caller_request_id)",
        "CREATE TABLE model_route_decisions",
        "UNIQUE (tenant_id,workspace_id,preparation_request_key)",
        "CREATE TABLE model_route_dispositions",
        "UNIQUE (tenant_id,workspace_id,decision_id)",
        "BEFORE UPDATE OR DELETE ON model_route_preparations",
        "BEFORE UPDATE OR DELETE ON model_route_decisions",
        "BEFORE UPDATE OR DELETE ON model_route_dispositions",
        "FORCE ROW LEVEL SECURITY",
        "observed_actual' IS NOT DISTINCT FROM 'null'::jsonb",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
}

#[test]
fn store_rechecks_current_work_and_never_records_execution() {
    for required in [
        ".approved_work_context(",
        "SELECT id FROM slice_candidate_sets",
        "FOR SHARE",
        "fresh.host_capabilities = work.host_capabilities.clone()",
        "if fresh != *work",
        "catalogue.eligible(work)",
        "prepared.routes.observed_actual.is_some()",
        "value.routes.observed_actual.is_some()",
        "value.prepared.routes.observed_actual.is_some()",
        "self.principal_id()? != value.actor_id",
    ] {
        assert!(STORE.contains(required), "missing {required}");
    }
    assert!(!STORE.contains("dispatch_model"));
    assert!(GRANTS.contains("model_route_preparations"));
    assert!(GRANTS.contains("model_route_decisions"));
    assert!(GRANTS.contains("model_route_dispositions"));
}
