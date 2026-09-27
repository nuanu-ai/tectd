const SQL: &str = include_str!("../migrations/0107_context_matrix_verification.sql");

#[test]
fn versioned_verification_keeps_v1_identity_and_binds_v2_to_frozen_revision() {
    assert!(SQL.contains("ADD COLUMN frozen_snapshot_id uuid"));
    assert!(SQL.contains("schema='tect.matrix-verification/1'"));
    assert!(SQL.contains("schema='tect.context-matrix-verification/1'"));
    assert!(SQL.contains("frozen_snapshot_id IS NULL"));
    assert!(SQL.contains("frozen_snapshot_id IS NOT NULL"));
    assert!(SQL.contains("requirements_semantic_digest IS NOT NULL"));
    assert!(SQL.contains("authority_schema IS NOT NULL"));
    assert!(SQL.contains("matrix_verifications_requirements_binding_fk"));
    assert!(SQL.contains("matrix_verifications_frozen_snapshot_fk"));
    assert!(!SQL.contains("DROP TABLE"));
    assert!(!SQL.contains("UPDATE public.matrix_verifications"));
}

#[test]
fn v2_no_call_reasons_only_allow_matrix_engineering_profile() {
    for reason in [
        "matrix_task_unbound",
        "matrix_snapshot_missing",
        "matrix_binding_mismatch",
        "matrix_context_unresolved",
        "matrix_context_stale",
        "matrix_authority_schema_unsupported",
        "matrix_operating_evidence_unresolved",
    ] {
        assert!(
            SQL.matches(reason).count() >= 2,
            "missing reason/state: {reason}"
        );
    }
    assert!(SQL.contains("AND capability='engineering_profile' AND work_item_kind='matrix_task'"));
}
