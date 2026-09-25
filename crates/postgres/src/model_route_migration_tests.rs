const MIGRATION: &str = include_str!("../migrations/0080_model_route_recommendation_receipts.sql");
const STORE: &str = include_str!("model_route_store.rs");
const GRANTS: &str = include_str!("admin/migration.rs");
const AUDIT: &str = include_str!("../migrations/0082_model_route_advisory_attempts.sql");
const ATTEMPTS: &str = include_str!("model_route_attempt_store.rs");
const ATTEMPT_BUDGET: &str = include_str!("model_route_attempt_store/budget.rs");
const ATTEMPT_READS: &str = include_str!("model_route_attempt_store/reads.rs");

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
fn optional_ranker_has_one_use_raw_fence_and_unified_audit() {
    for required in [
        "CREATE TABLE model_route_advisory_attempts",
        "UNIQUE (tenant_id,workspace_id,preparation_request_key)",
        "invoking_session_id uuid NOT NULL",
        "invoking_principal_id uuid NOT NULL",
        "NOT s.revoked AND NOT h.revoked AND pr.role='owner'",
        "state='send_unknown' AND NEW.state='raw_sealed'",
        "OLD.state='raw_sealed' AND NEW.state='parsed'",
        "pg_catalog.sha256(request_payload)",
        "pg_catalog.sha256(response_payload)",
        "FORCE ROW LEVEL SECURITY",
        "CREATE VIEW advisory_call_audit WITH (security_invoker=true)",
        "CASE WHEN a.state='no_call' THEN 0 ELSE 1 END",
    ] {
        assert!(AUDIT.contains(required), "missing {required}");
    }
    assert!(
        ATTEMPT_BUDGET.contains("attempted.verify(prepared)?"),
        "missing attempted.verify(prepared)?"
    );
    assert!(
        ATTEMPTS.contains("model_route_wire_sha256(raw) != digest"),
        "missing model_route_wire_sha256(raw) != digest"
    );
    assert!(
        ATTEMPT_READS
            .contains("parse_model_route_ranking_response(&attempted.request, &raw_response)?")
    );
    let flow = ATTEMPTS
        .split("async fn stored_preparation(")
        .nth(1)
        .expect("stored preparation guard exists")
        .split("async fn attempt_row(")
        .next()
        .unwrap();
    let checked = flow
        .split("current_preparation(uow, &")
        .nth(1)
        .and_then(|tail| tail.split_once(").await?"))
        .map(|(name, _)| name)
        .expect("current preparation is rechecked");
    let declaration = flow.find(&format!("let {checked} = uow")).unwrap();
    let lookup = flow.find(".by_request(").unwrap();
    let equality = flow.find(&format!("if {checked} != *prepared")).unwrap();
    let current = flow.find("current_preparation(uow, &").unwrap();
    assert!(declaration < lookup && lookup < equality && equality < current);
    assert!(STORE.contains(".sealed_provider_ranking(stored.workspace_id"));
    assert!(GRANTS.contains("model_route_advisory_attempts"));
    assert!(GRANTS.contains("advisory_call_audit"));
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
