const SQL: &str = include_str!("../migrations/0055_context_matrix_verification.sql");

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
