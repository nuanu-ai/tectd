const SQL: &str = include_str!("../migrations/0105_matrix_declared_requirements_context.sql");

#[test]
fn declared_requirements_context_is_append_only_tenant_bound_and_explicitly_confirmed() {
    for required in [
        "matrix_requirements_proposals",
        "matrix_requirements_confirmations",
        "matrix_requirements_snapshots",
        "FORCE ROW LEVEL SECURITY",
        "TG_OP<>'INSERT'",
        "owner_response_ref",
        "proposal_revision,proposal_digest",
        "FOR UPDATE OF n",
        "context revision conflict",
        "requires active owner session",
        "BETWEEN 1 AND 8388608",
        "pg_catalog.sha256(canonical_payload)",
        "IS TRUE",
    ] {
        assert!(SQL.contains(required), "missing {required}");
    }
    assert!(!SQL.contains("UPDATE public."));
    let grants = include_str!("admin/migration.rs");
    assert!(grants.contains("GRANT SELECT,INSERT ON TABLE matrix_requirements_proposals,matrix_requirements_confirmations,matrix_requirements_snapshots"));
    assert!(!grants.contains("GRANT UPDATE ON TABLE matrix_requirements"));
}

#[test]
fn requirements_lineage_uses_saved_work_and_exact_opening_origin() {
    let source = include_str!("matrix_requirements_context_store/lineage.rs");
    assert!(source.contains("origin.candidate_set_revision"));
    assert!(source.contains("origin.candidate_snapshot_id != snapshot"));
    assert!(source.contains("sets != vec![origin.candidate_set_id]"));
    assert!(source.contains("set_revision<=$4"));
    assert!(source.contains("SliceCandidateNode::Work"));
    assert!(source.contains("nodes[0].revision() != revision"));
    let store = include_str!("matrix_requirements_context_store.rs");
    assert!(store.contains("canonical_payload FROM matrix_requirements_snapshots"));
    assert!(store.contains("if saved != bytes"));
    assert!(store.contains("effective: trusted"));
}
